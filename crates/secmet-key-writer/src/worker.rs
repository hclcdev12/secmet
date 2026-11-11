//! Processus privilégié d'écriture (`--worker <disque> <image>`).
//!
//! Il décompresse l'image en flux, l'écrit sur le disque ouvert en
//! exclusif, synchronise, vide le cache des blocs puis relit tout le
//! contenu écrit et compare les empreintes SHA-256. Chaque étape est
//! annoncée par une ligne JSON ([`Event`]) sur la sortie standard ; la
//! fermeture de l'entrée standard (ou la ligne `cancel`) interrompt le
//! travail entre deux blocs.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use xz_native::{DecoderOptions, XzReader};

use crate::platform;

/// Taille des blocs écrits.
pub const CHUNK: usize = 4 << 20;
/// Alignement des tampons et des tailles d'écriture directe (multiple de
/// tout secteur logique, 512 ou 4096).
pub const ALIGN: usize = 4096;

/// Tampon aligné sur [`ALIGN`] pour l'écriture directe.
struct AlignedBuf {
    raw: Vec<u8>,
    offset: usize,
}

impl AlignedBuf {
    fn new(len: usize) -> Self {
        let raw = vec![0u8; len + ALIGN];
        let offset = (ALIGN - (raw.as_ptr() as usize % ALIGN)) % ALIGN;
        AlignedBuf { raw, offset }
    }

    fn slice(&mut self) -> &mut [u8] {
        let end = self.raw.len() - ALIGN + self.offset;
        &mut self.raw[self.offset..end]
    }
}

/// Arrondit à un multiple de [`ALIGN`].
fn round_up(n: u64) -> u64 {
    n.div_ceil(ALIGN as u64) * ALIGN as u64
}

/// Étapes du travail, dans l'ordre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Démontage, ouverture exclusive, contrôle de taille.
    Prepare,
    /// Décompression et écriture.
    Write,
    /// `fsync` et vidage du cache.
    Flush,
    /// Relecture et comparaison des empreintes.
    Verify,
    /// Relecture de la table de partitions.
    Finish,
}

impl Phase {
    /// Libellé affiché.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Prepare => "Préparation de la clé",
            Phase::Write => "Écriture",
            Phase::Flush => "Synchronisation",
            Phase::Verify => "Relecture et contrôle",
            Phase::Finish => "Finalisation",
        }
    }
}

/// Message du processus d'écriture vers l'interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Début d'une étape ; `total` octets à traiter (0 si sans progression).
    Phase { phase: Phase, total: u64 },
    /// Avancement dans l'étape courante.
    Progress { done: u64 },
    /// Information pour le journal.
    Info { message: String },
    /// Travail terminé : empreinte des octets écrits et relus.
    Done { sha256: String, written: u64 },
    /// Échec ; le processus se termine ensuite.
    Error { message: String },
}

fn emit(ev: &Event) {
    if let Ok(line) = serde_json::to_string(ev) {
        let out = std::io::stdout();
        let mut l = out.lock();
        let _ = writeln!(l, "{line}");
        let _ = l.flush();
    }
}

fn info(message: impl Into<String>) {
    emit(&Event::Info {
        message: message.into(),
    });
}

/// Surveille l'entrée standard : fermée ou `cancel` → annulation.
fn watch_stdin(cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) if l.trim() == "cancel" => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        cancel.store(true, Ordering::Relaxed);
    });
}

/// Point d'entrée de `--worker`.
pub fn run(device: &str, image: &str) -> ExitCode {
    let cancel = Arc::new(AtomicBool::new(false));
    watch_stdin(cancel.clone());
    match work(device, Path::new(image), &cancel) {
        Ok((sha256, written)) => {
            emit(&Event::Done { sha256, written });
            ExitCode::SUCCESS
        }
        Err(message) => {
            emit(&Event::Error { message });
            ExitCode::FAILURE
        }
    }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Écrit la zone de secours de la GPT (entrées puis en-tête, jusqu'au
/// dernier secteur) depuis un tampon aligné, puis revient au début.
fn write_tail(dev: &mut File, p: &crate::gpt::Plan) -> std::io::Result<()> {
    let mut aligned = AlignedBuf::new(p.tail.len());
    let buf = aligned.slice();
    buf.copy_from_slice(&p.tail);
    dev.seek(SeekFrom::Start(p.tail_lba * crate::gpt::SECTOR))?;
    dev.write_all(buf)?;
    dev.seek(SeekFrom::Start(0))?;
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("écriture annulée : la clé est incomplète, recommencer".to_string())
    } else {
        Ok(())
    }
}

/// Vrai si la cible est un fichier ordinaire (essais sans disque : les
/// ioctl de bloc sont alors remplacés ou ignorés).
fn is_regular_file(device: &str) -> bool {
    // Chemins de périphérique Windows : jamais des fichiers, quoi qu'en
    // dise `metadata`.
    if device.starts_with(r"\\.\") || device.starts_with(r"\\?\") {
        return false;
    }
    std::fs::metadata(device)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

fn work(device: &str, image: &Path, cancel: &AtomicBool) -> Result<(String, u64), String> {
    let file_target = is_regular_file(device);
    if !file_target && !platform::is_privileged() {
        return Err("droits insuffisants pour écrire sur le disque".to_string());
    }
    emit(&Event::Phase {
        phase: Phase::Prepare,
        total: 0,
    });
    let info_img = crate::image::inspect(image)?;
    let total = info_img.uncompressed_size;
    info(format!(
        "image {} : {} à écrire ({} blocs xz)",
        image.display(),
        crate::fmt_size(total),
        info_img.blocks
    ));
    let unmounted = platform::unmount_partitions(device).map_err(|e| e.to_string())?;
    for m in &unmounted {
        info(format!("démonté : {m}"));
    }
    let mut dev = platform::open_write(device)
        .map_err(|e| format!("ouverture exclusive de {device} impossible : {e}"))?;
    let size = if file_target {
        dev.metadata().map(|m| m.len()).map_err(|e| e.to_string())?
    } else {
        platform::device_size(&dev).map_err(|e| format!("taille de {device} : {e}"))?
    };
    if size < round_up(total) {
        return Err(format!(
            "{device} fait {} : l'image en demande {}",
            crate::fmt_size(size),
            crate::fmt_size(total)
        ));
    }
    info(format!("{device} : {}", crate::fmt_size(size)));
    check_cancel(cancel)?;

    emit(&Event::Phase {
        phase: Phase::Write,
        total,
    });
    // Décompression dans un fil producteur, écriture dans celui-ci : la
    // clé n'attend jamais le décodeur, et réciproquement. Trois blocs
    // d'avance au plus (12 Mio).
    let src = File::open(image).map_err(|e| format!("{} : {e}", image.display()))?;
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Vec<u8>, String>>(3);
    let producer_cancel = Arc::new(AtomicBool::new(false));
    let producer_flag = producer_cancel.clone();
    let producer = std::thread::spawn(move || {
        // Blocs xz indépendants (xz -T0 à la création) : décodage
        // parallèle, un fil par processeur, sortie identique au décodage
        // séquentiel.
        let mut opts = DecoderOptions::default();
        opts.threads = 0;
        let mut xz = XzReader::with_options(BufReader::with_capacity(1 << 20, src), opts);
        loop {
            if producer_flag.load(Ordering::Relaxed) {
                return;
            }
            let mut chunk = vec![0u8; CHUNK];
            match read_full(&mut xz, &mut chunk) {
                Ok(0) => return,
                Ok(n) => {
                    chunk.truncate(n);
                    if tx.send(Ok(chunk)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("décompression : {e}")));
                    return;
                }
            }
        }
    });
    let mut aligned = AlignedBuf::new(CHUNK);
    let buf = aligned.slice();
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut gpt_plan: Option<crate::gpt::Plan> = None;
    let outcome = loop {
        if let Err(e) = check_cancel(cancel) {
            break Err(e);
        }
        let chunk = match rx.recv() {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => break Err(e),
            Err(_) => break Ok(()),
        };
        let n = chunk.len();
        if written + n as u64 > total {
            break Err("l'image décompressée dépasse la taille annoncée par son index".to_string());
        }
        hasher.update(&chunk);
        // Tampon aligné pour l'écriture directe ; dernier bloc complété de
        // zéros jusqu'à l'alignement, zéros qui ne comptent ni dans
        // l'empreinte ni dans la relecture.
        let padded = round_up(n as u64) as usize;
        buf[..n].copy_from_slice(&chunk);
        buf[n..padded].fill(0);
        if written == 0 {
            // GPT complétée au bout du disque (voir `gpt`) : la copie de
            // secours part avant le premier bloc, puis les secteurs 0 et 1
            // corrigés partent avec lui.
            match crate::gpt::plan(&chunk, size) {
                Ok(Some(p)) => {
                    if let Err(e) = write_tail(&mut dev, &p) {
                        break Err(format!("GPT de secours sur {device} : {e}"));
                    }
                    buf[..p.head.len()].copy_from_slice(&p.head);
                    gpt_plan = Some(p);
                }
                Ok(None) => {}
                Err(e) => break Err(format!("GPT de l'image : {e}")),
            }
        }
        if let Err(e) = dev.write_all(&buf[..padded]) {
            break Err(format!(
                "écriture sur {device} après {} : {e}",
                crate::fmt_size(written)
            ));
        }
        written += n as u64;
        emit(&Event::Progress { done: written });
    };
    producer_cancel.store(true, Ordering::Relaxed);
    drop(rx);
    let _ = producer.join();
    outcome?;
    if written != total {
        return Err(format!(
            "image tronquée : {} décompressés, {} attendus",
            crate::fmt_size(written),
            crate::fmt_size(total)
        ));
    }
    let expected = secmet_attest::hex(&hasher.finalize());

    emit(&Event::Phase {
        phase: Phase::Flush,
        total: 0,
    });
    dev.sync_all()
        .map_err(|e| format!("synchronisation de {device} : {e}"))?;
    if !file_target {
        platform::drop_caches(&dev).map_err(|e| format!("vidage du cache de {device} : {e}"))?;
    }
    drop(dev);
    check_cancel(cancel)?;

    emit(&Event::Phase {
        phase: Phase::Verify,
        total,
    });
    let mut rd = platform::open_read(device).map_err(|e| format!("relecture de {device} : {e}"))?;
    let mut hasher = Sha256::new();
    let mut read = 0u64;
    while read < total {
        check_cancel(cancel)?;
        let want = ((total - read) as usize).min(CHUNK);
        // Lecture de taille alignée (lecture directe sous Windows ; le
        // disque fait au moins `round_up(total)`) ; seuls `want` octets
        // comptent.
        let ask = round_up(want as u64) as usize;
        let n = read_full(&mut rd, &mut buf[..ask])
            .map_err(|e| format!("relecture de {device} : {e}"))?;
        if n < want {
            return Err(format!(
                "relecture interrompue à {}",
                crate::fmt_size(read + n as u64)
            ));
        }
        if read == 0 {
            if let Some(p) = &gpt_plan {
                // Secteurs 0 et 1 complétés : comparés tels quels, puis
                // remplacés par ceux de l'image pour l'empreinte.
                if buf[..p.head.len()] != p.head[..] {
                    return Err(
                        "la relecture de la table de partitions ne correspond pas à ce qui a été écrit"
                            .to_string(),
                    );
                }
                buf[..p.pristine_head.len()].copy_from_slice(&p.pristine_head);
            }
        }
        hasher.update(&buf[..want]);
        read += want as u64;
        emit(&Event::Progress { done: read });
    }
    let actual = secmet_attest::hex(&hasher.finalize());
    if actual != expected {
        return Err(
            "la relecture ne correspond pas à ce qui a été écrit : clé défectueuse ou débranchée"
                .to_string(),
        );
    }
    if let Some(p) = &gpt_plan {
        rd.seek(SeekFrom::Start(p.tail_lba * crate::gpt::SECTOR))
            .map_err(|e| format!("relecture de {device} : {e}"))?;
        let n = read_full(&mut rd, &mut buf[..p.tail.len()])
            .map_err(|e| format!("relecture de la GPT de secours sur {device} : {e}"))?;
        if n < p.tail.len() || buf[..p.tail.len()] != p.tail[..] {
            return Err(
                "la relecture de la GPT de secours ne correspond pas à ce qui a été écrit"
                    .to_string(),
            );
        }
        info(&p.message);
    }

    emit(&Event::Phase {
        phase: Phase::Finish,
        total: 0,
    });
    if !file_target {
        match platform::reread_partitions(&rd) {
            Ok(()) => info("table de partitions relue par le système"),
            Err(e) => info(format!(
                "table de partitions non relue ({e}) : rebrancher la clé"
            )),
        }
    }
    Ok((actual, written))
}
