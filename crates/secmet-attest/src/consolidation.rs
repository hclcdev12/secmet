//! Regroupement de lots clôturés : plusieurs clés SecMet traitent une même
//! mission (par exemple 100 portables sur 5 clés, un lot par clé), et le
//! client veut un seul livrable.
//!
//! Le regroupement **ne ré-atteste rien** et n'est pas signé (phase 1) :
//! il vérifie chaque lot (manifeste signé, attestations, empreintes),
//! recopie les fichiers d'origine tels quels dans `sources/<clé>/<lot>/`
//! et produit une synthèse (PDF et index JSON). La preuve reste celle des
//! lots : chaque manifeste et chaque attestation se vérifie avec
//! `secmet-verify`, comme avant le regroupement.
//!
//! Règles :
//! - seul un lot clôturé (manifeste signé présent) est accepté ;
//! - un même lot ajouté deux fois n'est compté qu'une fois ; deux lots de
//!   même identifiant mais de contenus différents sont refusés ;
//! - un disque (même numéro de série et même modèle) présent dans plusieurs
//!   lots, typiquement un portable repassé sur une autre clé : le dernier
//!   effacement réussi compte, les autres sont marqués « repassé » ;
//! - des références client différentes donnent un avertissement.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::export::safe_filename;
use crate::manifest::{self, SignedManifest};
use crate::model::{Disposition, Outcome};
use crate::sign::{self, SignedAttestation};
use crate::{sha256_file_hex, sha256_hex};

/// Suffixe du manifeste signé d'un lot (`<LOT-ID>-manifest.json`).
pub const MANIFEST_SUFFIX: &str = "-manifest.json";
/// Dossier des fichiers d'origine dans un regroupement.
pub const SOURCES_DIR: &str = "sources";
/// Version du format de l'index JSON.
pub const INDEX_VERSION: u32 = 1;

/// Un fichier d'un lot source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Nom du fichier dans le dossier du lot.
    pub name: String,
    /// Chemin d'origine.
    pub path: PathBuf,
    /// SHA-256 du contenu.
    pub sha256: String,
}

/// Un lot clôturé, vérifié.
#[derive(Debug, Clone)]
pub struct SourceLot {
    /// Dossier d'origine (`SECMET-EXP/<LOT>/`).
    pub dir: PathBuf,
    /// Manifeste signé.
    pub manifest: SignedManifest,
    /// Attestations, dans l'ordre du manifeste.
    pub certificates: Vec<SignedAttestation>,
    /// Fichiers du dossier, recopiés tels quels.
    pub files: Vec<SourceFile>,
}

impl SourceLot {
    /// Identifiant du lot.
    pub fn lot_id(&self) -> &str {
        &self.manifest.payload.lot_id
    }

    /// Identifiant de la clé qui a signé le lot.
    pub fn key_id(&self) -> &str {
        &self.manifest.key_id
    }

    /// Référence client, sans espaces de bord.
    pub fn reference(&self) -> &str {
        self.manifest.payload.reference.trim()
    }
}

/// Nom du fichier de manifeste dans `dir`, s'il y en a exactement un.
fn find_manifest(dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut found = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(MANIFEST_SUFFIX) && entry.path().is_file() {
            found.push(entry.path());
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(format!(
            "{} : plusieurs manifestes dans le même dossier",
            dir.display()
        )),
    }
}

/// Lit et vérifie un lot clôturé : signature du manifeste, signature de
/// chaque attestation, même clé partout, aucune attestation manquante,
/// modifiée ou en trop.
pub fn load_lot(dir: &Path) -> Result<SourceLot, String> {
    let Some(manifest_path) = find_manifest(dir)? else {
        return Err(format!(
            "{} : aucun manifeste de lot. Seul un lot clôturé peut être regroupé : \
             clôturez-le dans SecMet.",
            dir.display()
        ));
    };
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{} : {e}", manifest_path.display()))?;
    let signed: SignedManifest = serde_json::from_str(&text)
        .map_err(|e| format!("{} : manifeste illisible ({e})", manifest_path.display()))?;
    manifest::verify_manifest(&signed, &[])
        .map_err(|e| format!("{} : {e}", manifest_path.display()))?;

    let mut certificates = Vec::with_capacity(signed.payload.certificates.len());
    for entry in &signed.payload.certificates {
        let path = dir.join(format!("{}.json", safe_filename(&entry.certificate_id)));
        let text = fs::read_to_string(&path).map_err(|e| {
            format!(
                "{} : certificat {} manquant ({e})",
                dir.display(),
                entry.certificate_id
            )
        })?;
        let envelope: SignedAttestation = serde_json::from_str(&text)
            .map_err(|e| format!("{} : certificat illisible ({e})", path.display()))?;
        sign::verify(&envelope, &[]).map_err(|e| format!("{} : {e}", path.display()))?;
        if envelope.key_id != signed.key_id || envelope.public_key != signed.public_key {
            return Err(format!(
                "{} : signé par une autre clé que le manifeste du lot",
                path.display()
            ));
        }
        certificates.push(envelope);
    }
    let problems =
        manifest::reconcile(&signed.payload, &certificates).map_err(|e| e.to_string())?;
    if let Some(first) = problems.first() {
        return Err(format!("{} : {first}", dir.display()));
    }

    let mut files = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.is_file() || name.starts_with('.') || name.ends_with(".tmp") {
            continue;
        }
        let sha256 = sha256_file_hex(&path).map_err(|e| format!("{} : {e}", path.display()))?;
        files.push(SourceFile { name, path, sha256 });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(SourceLot {
        dir: dir.to_path_buf(),
        manifest: signed,
        certificates,
        files,
    })
}

/// Résultat de la recherche de lots sous un dossier.
#[derive(Debug, Default)]
pub struct Discovered {
    /// Lots clôturés et vérifiés.
    pub lots: Vec<SourceLot>,
    /// Dossiers de lot refusés, avec la raison.
    pub refused: Vec<String>,
}

/// Lots trouvés dans `dir` : le dossier lui-même s'il contient un
/// manifeste, sinon ses sous-dossiers directs qui en contiennent un (la
/// partition SECMET-EXP d'une clé entière). Les sous-dossiers sans
/// manifeste (certificats hors lot) sont ignorés.
pub fn discover(dir: &Path) -> Discovered {
    let mut out = Discovered::default();
    match find_manifest(dir) {
        Ok(Some(_)) => {
            match load_lot(dir) {
                Ok(lot) => out.lots.push(lot),
                Err(e) => out.refused.push(e),
            }
            return out;
        }
        Ok(None) => {}
        Err(e) => {
            out.refused.push(e);
            return out;
        }
    }
    let mut subdirs: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => {
            out.refused.push(format!("{} : {e}", dir.display()));
            return out;
        }
    };
    subdirs.sort();
    for sub in subdirs {
        match find_manifest(&sub) {
            Ok(Some(_)) => match load_lot(&sub) {
                Ok(lot) => out.lots.push(lot),
                Err(e) => out.refused.push(e),
            },
            Ok(None) => {}
            Err(e) => out.refused.push(e),
        }
    }
    if out.lots.is_empty() && out.refused.is_empty() {
        out.refused.push(format!(
            "{} : aucun lot clôturé dans ce dossier",
            dir.display()
        ));
    }
    out
}

/// Sort d'un certificat dans le regroupement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStatus {
    /// Compté dans le bilan.
    Counted,
    /// Disque repassé : un autre certificat (`by`) compte à sa place.
    Redone {
        /// `certificate_id` du certificat retenu.
        by: String,
    },
}

/// Un certificat du regroupement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Indice du lot dans [`Grouping::lots`].
    pub lot: usize,
    /// Indice du certificat dans le lot.
    pub cert: usize,
    /// Compté ou repassé.
    pub status: EntryStatus,
}

/// Bilan chiffré.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    /// Disques comptés (un par disque, repassages exclus).
    pub disks: usize,
    /// Comptés et réussis.
    pub success: usize,
    /// Comptés et échoués ou interrompus.
    pub failed: usize,
    /// Certificats non comptés car le disque a été repassé.
    pub redone: usize,
    /// Comptés et à détruire physiquement.
    pub destroy: usize,
    /// Disques sans numéro de série (aucun dédoublonnage possible).
    pub no_serial: usize,
}

/// Regroupement prêt à écrire.
#[derive(Debug, Clone)]
pub struct Grouping {
    /// Lots, du plus ancien au plus récent (ouverture).
    pub lots: Vec<SourceLot>,
    /// Certificats, lot par lot dans l'ordre des manifestes.
    pub entries: Vec<Entry>,
    /// Avertissements (n'empêchent pas le regroupement).
    pub warnings: Vec<String>,
}

/// Instant d'un certificat pour départager deux passages : fin de
/// l'opération, sinon émission. Format `AAAA-MM-JJ HH:MM:SS` : l'ordre
/// lexical est l'ordre chronologique.
fn instant(a: &SignedAttestation) -> &str {
    a.payload
        .erasure
        .finished_at
        .as_deref()
        .unwrap_or(&a.payload.issued_at)
}

/// Clé d'identité d'un disque : modèle et série, sans espaces de bord ni
/// casse ; `None` sans série.
fn disk_key(a: &SignedAttestation) -> Option<(String, String)> {
    let serial = a.payload.disk.serial.trim();
    if serial.is_empty() {
        return None;
    }
    Some((
        a.payload.disk.model.trim().to_lowercase(),
        serial.to_lowercase(),
    ))
}

/// Construit le regroupement : contrôle les lots, écarte un lot ajouté deux
/// fois, résout les disques repassés.
pub fn build(mut lots: Vec<SourceLot>) -> Result<Grouping, String> {
    if lots.is_empty() {
        return Err("aucun lot à regrouper".into());
    }
    let mut warnings = Vec::new();

    // Même lot ajouté deux fois : un seul ; même identifiant (sous la même
    // clé) avec un autre contenu : refus.
    let mut unique: Vec<SourceLot> = Vec::with_capacity(lots.len());
    for lot in lots.drain(..) {
        match unique
            .iter()
            .find(|u| u.key_id() == lot.key_id() && u.lot_id() == lot.lot_id())
        {
            Some(u) if u.manifest.signature == lot.manifest.signature => {
                warnings.push(format!(
                    "lot {} ajouté deux fois ({} et {}) : compté une seule fois",
                    lot.lot_id(),
                    u.dir.display(),
                    lot.dir.display()
                ));
            }
            Some(u) => {
                return Err(format!(
                    "lot {} présent deux fois avec des contenus différents ({} et {})",
                    lot.lot_id(),
                    u.dir.display(),
                    lot.dir.display()
                ));
            }
            None => unique.push(lot),
        }
    }
    let mut lots = unique;
    lots.sort_by(|a, b| {
        a.manifest
            .payload
            .opened_at
            .cmp(&b.manifest.payload.opened_at)
            .then_with(|| a.lot_id().cmp(b.lot_id()))
    });

    // Un certificat ne peut figurer que dans un lot.
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for lot in &lots {
        for c in &lot.certificates {
            if let Some(other) = seen.insert(&c.payload.certificate_id, lot.lot_id()) {
                return Err(format!(
                    "certificat {} présent dans deux lots ({other} et {})",
                    c.payload.certificate_id,
                    lot.lot_id()
                ));
            }
        }
    }

    let references: BTreeSet<&str> = lots.iter().map(SourceLot::reference).collect();
    if references.len() > 1 {
        let shown: Vec<String> = references
            .iter()
            .map(|r| {
                if r.is_empty() {
                    "(aucune)".to_string()
                } else {
                    format!("« {r} »")
                }
            })
            .collect();
        warnings.push(format!(
            "références client différentes : {} ; vérifiez que ces lots appartiennent à la même mission",
            shown.join(", ")
        ));
    } else if references.iter().all(|r| r.is_empty()) && lots.len() > 1 {
        warnings.push(
            "aucune référence client : rien ne permet de vérifier que ces lots appartiennent \
             à la même mission"
                .into(),
        );
    }
    let policies: BTreeSet<&str> = lots
        .iter()
        .flat_map(|l| l.certificates.iter())
        .map(|c| c.payload.erasure.policy.as_str())
        .collect();
    if policies.len() > 1 {
        let list: Vec<&str> = policies.into_iter().collect();
        warnings.push(format!(
            "politiques de classement différentes selon les lots : {}",
            list.join(", ")
        ));
    }

    // Disques repassés : le dernier succès compte, sinon le dernier passage.
    let mut groups: HashMap<(String, String), Vec<(usize, usize)>> = HashMap::new();
    for (li, lot) in lots.iter().enumerate() {
        for (ci, c) in lot.certificates.iter().enumerate() {
            if let Some(key) = disk_key(c) {
                groups.entry(key).or_default().push((li, ci));
            }
        }
    }
    let mut redone: HashMap<(usize, usize), String> = HashMap::new();
    for members in groups.values().filter(|m| m.len() > 1) {
        let cert = |&(li, ci): &(usize, usize)| &lots[li].certificates[ci];
        let winner = members
            .iter()
            .max_by(|a, b| {
                let (ca, cb) = (cert(a), cert(b));
                let sa = ca.payload.erasure.outcome == Outcome::Success;
                let sb = cb.payload.erasure.outcome == Outcome::Success;
                sa.cmp(&sb).then_with(|| instant(ca).cmp(instant(cb)))
            })
            .copied();
        if let Some(w) = winner {
            let by = cert(&w).payload.certificate_id.clone();
            for m in members.iter().filter(|m| **m != w) {
                redone.insert(*m, by.clone());
            }
        }
    }

    let entries = lots
        .iter()
        .enumerate()
        .flat_map(|(li, lot)| (0..lot.certificates.len()).map(move |ci| (li, ci)))
        .map(|(li, ci)| Entry {
            lot: li,
            cert: ci,
            status: match redone.get(&(li, ci)) {
                Some(by) => EntryStatus::Redone { by: by.clone() },
                None => EntryStatus::Counted,
            },
        })
        .collect();
    let grouping = Grouping {
        lots,
        entries,
        warnings,
    };
    let no_serial = grouping.totals().no_serial;
    let mut grouping = grouping;
    if no_serial > 0 {
        grouping.warnings.push(if no_serial == 1 {
            "1 disque sans numéro de série : un éventuel repassage ne peut pas être repéré".into()
        } else {
            format!(
                "{no_serial} disques sans numéro de série : un éventuel repassage ne peut pas être repéré"
            )
        });
    }
    Ok(grouping)
}

impl Grouping {
    /// Certificat d'une entrée.
    pub fn certificate(&self, e: &Entry) -> &SignedAttestation {
        &self.lots[e.lot].certificates[e.cert]
    }

    /// Bilan chiffré.
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for e in &self.entries {
            let a = &self.certificate(e).payload;
            if a.disk.serial.trim().is_empty() {
                t.no_serial += 1;
            }
            if e.status != EntryStatus::Counted {
                t.redone += 1;
                continue;
            }
            t.disks += 1;
            if a.erasure.outcome == Outcome::Success {
                t.success += 1;
            } else {
                t.failed += 1;
            }
            if a.erasure.disposition.is_some_and(Disposition::destroy) {
                t.destroy += 1;
            }
        }
        t
    }

    /// Clés de signature distinctes.
    pub fn key_ids(&self) -> Vec<&str> {
        let set: BTreeSet<&str> = self.lots.iter().map(SourceLot::key_id).collect();
        set.into_iter().collect()
    }

    /// Opérateurs distincts (ouverture des lots).
    pub fn operators(&self) -> Vec<&str> {
        let set: BTreeSet<&str> = self
            .lots
            .iter()
            .map(|l| l.manifest.payload.operator.trim())
            .collect();
        set.into_iter().collect()
    }

    /// Référence client commune (vide si absente ou différente selon les
    /// lots).
    pub fn reference(&self) -> String {
        let set: BTreeSet<&str> = self.lots.iter().map(SourceLot::reference).collect();
        if set.len() == 1 {
            set.into_iter().next().unwrap_or_default().to_string()
        } else {
            String::new()
        }
    }

    /// Première et dernière opération (`AAAA-MM-JJ HH:MM:SS`).
    pub fn period(&self) -> Option<(String, String)> {
        let certs = self.lots.iter().flat_map(|l| l.certificates.iter());
        let start = certs
            .clone()
            .filter_map(|c| {
                c.payload
                    .erasure
                    .started_at
                    .clone()
                    .or_else(|| Some(c.payload.issued_at.clone()))
            })
            .min()?;
        let end = certs.map(|c| instant(c).to_string()).max()?;
        Some((start, end))
    }

    /// Identifiant du regroupement : `R-AAAAMMJJ-XXXXXX`, la date de
    /// génération et une empreinte des manifestes regroupés (le même jeu
    /// de lots donne le même identifiant).
    pub fn id(&self, generated_at: &str) -> String {
        let date: String = generated_at
            .chars()
            .filter(char::is_ascii_digit)
            .take(8)
            .collect();
        let mut signatures: Vec<&str> = self
            .lots
            .iter()
            .map(|l| l.manifest.signature.as_str())
            .collect();
        signatures.sort_unstable();
        let digest = sha256_hex(signatures.join("\n").as_bytes());
        format!(
            "R-{date}-{}",
            digest.get(..6).unwrap_or_default().to_uppercase()
        )
    }

    /// Chemin relatif d'un fichier source dans le regroupement.
    pub fn source_path(&self, lot: usize, file: &SourceFile) -> String {
        let l = &self.lots[lot];
        format!(
            "{SOURCES_DIR}/{}/{}/{}",
            safe_filename(l.key_id()),
            safe_filename(l.lot_id()),
            file.name
        )
    }
}

/// Index JSON du regroupement (non signé) : ce que contient le dossier,
/// pour une relecture automatique.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// Version du format.
    pub version: u32,
    /// Identifiant du regroupement.
    pub grouping_id: String,
    /// Génération, `AAAA-MM-JJ HH:MM:SS`.
    pub generated_at: String,
    /// Logiciel qui a regroupé.
    pub software: String,
    /// Toujours `false` en phase 1 : la preuve est dans les lots.
    pub signed: bool,
    /// Lots regroupés.
    pub lots: Vec<IndexLot>,
    /// Certificats.
    pub entries: Vec<IndexEntry>,
    /// Avertissements affichés au regroupement.
    pub warnings: Vec<String>,
}

/// Lot dans l'index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexLot {
    /// Identifiant du lot.
    pub lot_id: String,
    /// Clé qui a signé le lot.
    pub key_id: String,
    /// Référence client.
    pub reference: String,
    /// Opérateur.
    pub operator: String,
    /// Clôture.
    pub closed_at: String,
    /// SHA-256 du fichier manifeste.
    pub manifest_sha256: String,
    /// Fichiers recopiés : chemin relatif et SHA-256.
    pub files: Vec<IndexFile>,
}

/// Fichier recopié.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexFile {
    /// Chemin relatif au dossier du regroupement.
    pub path: String,
    /// SHA-256.
    pub sha256: String,
}

/// Certificat dans l'index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Identifiant du certificat.
    pub certificate_id: String,
    /// Lot.
    pub lot_id: String,
    /// Numéro de série du disque.
    pub disk_serial: String,
    /// `counted` ou `redone`.
    pub status: String,
    /// Pour un disque repassé : certificat retenu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
}

impl Grouping {
    /// Index du regroupement.
    pub fn index(&self, generated_at: &str, software: &str) -> Index {
        Index {
            version: INDEX_VERSION,
            grouping_id: self.id(generated_at),
            generated_at: generated_at.to_string(),
            software: software.to_string(),
            signed: false,
            lots: self
                .lots
                .iter()
                .enumerate()
                .map(|(li, l)| IndexLot {
                    lot_id: l.lot_id().to_string(),
                    key_id: l.key_id().to_string(),
                    reference: l.reference().to_string(),
                    operator: l.manifest.payload.operator.clone(),
                    closed_at: l.manifest.payload.closed_at.clone(),
                    manifest_sha256: l
                        .files
                        .iter()
                        .find(|f| f.name.ends_with(MANIFEST_SUFFIX))
                        .map(|f| f.sha256.clone())
                        .unwrap_or_default(),
                    files: l
                        .files
                        .iter()
                        .map(|f| IndexFile {
                            path: self.source_path(li, f),
                            sha256: f.sha256.clone(),
                        })
                        .collect(),
                })
                .collect(),
            entries: self
                .entries
                .iter()
                .map(|e| {
                    let a = &self.certificate(e).payload;
                    let (status, replaced_by) = match &e.status {
                        EntryStatus::Counted => ("counted", None),
                        EntryStatus::Redone { by } => ("redone", Some(by.clone())),
                    };
                    IndexEntry {
                        certificate_id: a.certificate_id.clone(),
                        lot_id: self.lots[e.lot].lot_id().to_string(),
                        disk_serial: a.disk.serial.clone(),
                        status: status.to_string(),
                        replaced_by,
                    }
                })
                .collect(),
            warnings: self.warnings.clone(),
        }
    }
}

/// Ce qui a été écrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Dossier du regroupement.
    pub dir: PathBuf,
    /// PDF de synthèse.
    pub pdf: PathBuf,
    /// Index JSON.
    pub index: PathBuf,
}

/// Écrit le regroupement dans `<parent>/<id>/` : fichiers d'origine
/// recopiés (empreinte contrôlée après copie), index JSON, PDF de synthèse.
/// Le dossier est d'abord construit sous un nom temporaire puis renommé :
/// il est complet ou absent. Refuse d'écraser un regroupement existant.
pub fn write(
    grouping: &Grouping,
    parent: &Path,
    generated_at: &str,
    software: &str,
    pdf_bytes: impl FnOnce(&Index) -> crate::Result<Vec<u8>>,
) -> Result<Written, String> {
    let id = grouping.id(generated_at);
    let dir = parent.join(&id);
    if dir.exists() {
        return Err(format!(
            "{} existe déjà : ces lots ont déjà été regroupés ici",
            dir.display()
        ));
    }
    let tmp = parent.join(format!(".{id}.tmp"));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("{} : {e}", tmp.display()))?;
    }
    let result = (|| -> Result<(), String> {
        for (li, lot) in grouping.lots.iter().enumerate() {
            for file in &lot.files {
                let target = tmp.join(grouping.source_path(li, file));
                if let Some(p) = target.parent() {
                    fs::create_dir_all(p).map_err(|e| format!("{} : {e}", p.display()))?;
                }
                fs::copy(&file.path, &target)
                    .map_err(|e| format!("{} : {e}", file.path.display()))?;
                let copied =
                    sha256_file_hex(&target).map_err(|e| format!("{} : {e}", target.display()))?;
                if copied != file.sha256 {
                    return Err(format!(
                        "{} : la copie diffère de l'original (support défaillant, ou fichier modifié pendant le regroupement)",
                        file.path.display()
                    ));
                }
            }
        }
        let index = grouping.index(generated_at, software);
        let json = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
        fs::write(tmp.join(format!("{id}.json")), json)
            .map_err(|e| format!("{} : {e}", tmp.display()))?;
        let pdf = pdf_bytes(&index).map_err(|e| e.to_string())?;
        fs::write(tmp.join(format!("{id}.pdf")), pdf)
            .map_err(|e| format!("{} : {e}", tmp.display()))?;
        fs::rename(&tmp, &dir).map_err(|e| format!("{} : {e}", dir.display()))
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    Ok(Written {
        pdf: dir.join(format!("{id}.pdf")),
        index: dir.join(format!("{id}.json")),
        dir,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::manifest::{entry_for, sign_manifest, LotManifest};
    use crate::model::tests::sample;
    use crate::sign::SigningIdentity;

    /// Écrit un lot clôturé de `disks` (série, succès, fin) signé par la
    /// clé `seed` dans `root/<lot_id>/`, comme la clôture de SecMet.
    pub fn write_lot(
        root: &Path,
        seed: u8,
        lot_id: &str,
        reference: &str,
        disks: &[(&str, bool, &str)],
    ) -> PathBuf {
        let identity = SigningIdentity::from_seed(&[seed; 32]);
        let dir = root.join(lot_id);
        fs::create_dir_all(&dir).unwrap();
        let mut signed = Vec::new();
        for (i, (serial, ok, finished)) in disks.iter().enumerate() {
            let mut a = sample();
            a.certificate_id = format!("SEC-{lot_id}-{i}-sda");
            a.session_id = format!("SEC-{lot_id}-{i}");
            a.disk.serial = (*serial).into();
            a.erasure.finished_at = Some((*finished).into());
            a.issued_at = (*finished).into();
            if !ok {
                a.erasure.outcome = Outcome::Failed;
                a.erasure.nist_level = crate::model::NistLevel::NotClassified;
            }
            let env = identity.sign(&a).unwrap();
            fs::write(
                dir.join(format!("{}.json", safe_filename(&a.certificate_id))),
                serde_json::to_string_pretty(&env).unwrap(),
            )
            .unwrap();
            fs::write(dir.join(format!("{}.pdf", a.session_id)), b"%PDF session").unwrap();
            signed.push(env);
        }
        fs::write(dir.join(format!("{lot_id}-lot.pdf")), b"%PDF lot").unwrap();
        let payload = LotManifest {
            version: manifest::SCHEMA_VERSION,
            lot_id: lot_id.into(),
            operator: "Hugo".into(),
            reference: reference.into(),
            opened_at: format!("2026-11-26 0{seed}:00:00"),
            closed_at: "2026-11-26 18:00:00".into(),
            software: "secmet 0.5.1".into(),
            machines: disks.len() as u32,
            certificates: signed.iter().map(|s| entry_for(s).unwrap()).collect(),
            journal_sha256: "0".repeat(64),
        };
        let m = sign_manifest(&identity, &payload).unwrap();
        fs::write(
            dir.join(format!("{lot_id}{MANIFEST_SUFFIX}")),
            serde_json::to_string_pretty(&m).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join(format!("cle-publique-secmet-{}.pub", identity.key_id())),
            identity.public_key_hex(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_closed_lot_loads_and_tampering_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_lot(
            tmp.path(),
            1,
            "L-A",
            "Client X",
            &[("S1", true, "2026-11-26 10:00:00")],
        );
        let lot = load_lot(&dir).unwrap();
        assert_eq!(lot.lot_id(), "L-A");
        assert_eq!(lot.certificates.len(), 1);
        // Manifeste, PDF du lot, attestation, PDF de session, clé publique.
        assert_eq!(lot.files.len(), 5);

        // Attestation modifiée : signature invalide.
        let cert = dir.join("SEC-L-A-0-sda.json");
        let text = fs::read_to_string(&cert).unwrap();
        fs::write(&cert, text.replace("\"S1\"", "\"S2\"")).unwrap();
        assert!(load_lot(&dir).is_err());
        // Attestation retirée.
        fs::remove_file(&cert).unwrap();
        assert!(load_lot(&dir).unwrap_err().contains("manquant"));
    }

    #[test]
    fn an_open_lot_or_a_plain_folder_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("L-OPEN");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SEC-1-sda.json"), b"{}").unwrap();
        assert!(load_lot(&dir).unwrap_err().contains("clôturé"));
        let found = discover(tmp.path());
        assert!(found.lots.is_empty());
        assert_eq!(found.refused.len(), 1);
    }

    #[test]
    fn discover_takes_a_whole_export_partition() {
        let tmp = tempfile::tempdir().unwrap();
        write_lot(
            tmp.path(),
            1,
            "L-A",
            "X",
            &[("S1", true, "2026-11-26 10:00:00")],
        );
        write_lot(
            tmp.path(),
            1,
            "L-B",
            "X",
            &[("S2", true, "2026-11-26 11:00:00")],
        );
        // Certificat hors lot : ignoré.
        fs::create_dir_all(tmp.path().join("SEC-20261126-ZZZZ")).unwrap();
        let found = discover(tmp.path());
        assert_eq!(found.lots.len(), 2);
        assert!(found.refused.is_empty());
        // Le dossier d'un lot donné directement.
        assert_eq!(discover(&tmp.path().join("L-A")).lots.len(), 1);
    }

    #[test]
    fn redone_disk_counts_once_last_success_wins() {
        let tmp = tempfile::tempdir().unwrap();
        // Clé 1 : S1 échoue à 10 h ; clé 2 : S1 réussit à 14 h.
        let a = write_lot(
            tmp.path(),
            1,
            "L-A",
            "Mission",
            &[
                ("S1", false, "2026-11-26 10:00:00"),
                ("S2", true, "2026-11-26 10:30:00"),
            ],
        );
        let b = write_lot(
            tmp.path(),
            2,
            "L-B",
            "Mission",
            &[
                ("S1", true, "2026-11-26 14:00:00"),
                ("S3", true, "2026-11-26 14:30:00"),
            ],
        );
        let g = build(vec![load_lot(&a).unwrap(), load_lot(&b).unwrap()]).unwrap();
        let t = g.totals();
        assert_eq!(t.disks, 3);
        assert_eq!(t.success, 3);
        assert_eq!(t.failed, 0);
        assert_eq!(t.redone, 1);
        let redone = g
            .entries
            .iter()
            .find(|e| e.status != EntryStatus::Counted)
            .unwrap();
        assert_eq!(
            g.certificate(redone).payload.certificate_id,
            "SEC-L-A-0-sda"
        );
        assert_eq!(
            redone.status,
            EntryStatus::Redone {
                by: "SEC-L-B-0-sda".into()
            }
        );
        assert_eq!(g.key_ids().len(), 2);
        assert_eq!(g.reference(), "Mission");
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    }

    #[test]
    fn a_later_failure_does_not_beat_an_earlier_success() {
        let tmp = tempfile::tempdir().unwrap();
        let a = write_lot(
            tmp.path(),
            1,
            "L-A",
            "M",
            &[("S1", true, "2026-11-26 10:00:00")],
        );
        let b = write_lot(
            tmp.path(),
            2,
            "L-B",
            "M",
            &[("S1", false, "2026-11-26 15:00:00")],
        );
        let g = build(vec![load_lot(&a).unwrap(), load_lot(&b).unwrap()]).unwrap();
        let counted: Vec<_> = g
            .entries
            .iter()
            .filter(|e| e.status == EntryStatus::Counted)
            .collect();
        assert_eq!(counted.len(), 1);
        assert_eq!(
            g.certificate(counted[0]).payload.certificate_id,
            "SEC-L-A-0-sda"
        );
        assert_eq!(g.totals().failed, 0);
    }

    #[test]
    fn same_lot_twice_counts_once_but_conflicting_copies_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let a = write_lot(
            tmp.path(),
            1,
            "L-A",
            "M",
            &[("S1", true, "2026-11-26 10:00:00")],
        );
        let g = build(vec![load_lot(&a).unwrap(), load_lot(&a).unwrap()]).unwrap();
        assert_eq!(g.lots.len(), 1);
        assert_eq!(g.warnings.len(), 1);
        // Même identifiant, même clé, autre contenu.
        let other = tempfile::tempdir().unwrap();
        let b = write_lot(
            other.path(),
            1,
            "L-A",
            "M",
            &[("S9", true, "2026-11-26 11:00:00")],
        );
        assert!(build(vec![load_lot(&a).unwrap(), load_lot(&b).unwrap()])
            .unwrap_err()
            .contains("contenus différents"));
    }

    #[test]
    fn different_references_warn() {
        let tmp = tempfile::tempdir().unwrap();
        let a = write_lot(
            tmp.path(),
            1,
            "L-A",
            "Client X",
            &[("S1", true, "2026-11-26 10:00:00")],
        );
        let b = write_lot(
            tmp.path(),
            2,
            "L-B",
            "Client Y",
            &[("S2", true, "2026-11-26 11:00:00")],
        );
        let g = build(vec![load_lot(&a).unwrap(), load_lot(&b).unwrap()]).unwrap();
        assert!(g.warnings[0].contains("références client différentes"));
        assert_eq!(g.reference(), "");
    }

    #[test]
    fn write_copies_sources_verbatim_and_refuses_to_overwrite() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let a = write_lot(
            src.path(),
            1,
            "L-A",
            "M",
            &[("S1", true, "2026-11-26 10:00:00")],
        );
        let b = write_lot(
            src.path(),
            2,
            "L-B",
            "M",
            &[("S2", true, "2026-11-26 11:00:00")],
        );
        let g = build(vec![load_lot(&a).unwrap(), load_lot(&b).unwrap()]).unwrap();
        let w = write(
            &g,
            out.path(),
            "2026-11-27 09:00:00",
            "secmet-consolidate 0.5.1",
            |_| Ok(b"%PDF".to_vec()),
        )
        .unwrap();
        assert!(w.pdf.is_file());
        assert!(w
            .dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("R-20261127-"));
        // Les sources recopiées restent des lots valides.
        let key_dir = w
            .dir
            .join(SOURCES_DIR)
            .join(safe_filename(g.lots[0].key_id()));
        let reloaded = load_lot(&key_dir.join("L-A")).unwrap();
        assert_eq!(reloaded.files, {
            let mut f = g.lots[0].files.clone();
            for (x, y) in f.iter_mut().zip(&reloaded.files) {
                x.path = y.path.clone();
            }
            f
        });
        let index: Index = serde_json::from_str(&fs::read_to_string(&w.index).unwrap()).unwrap();
        assert!(!index.signed);
        assert_eq!(index.lots.len(), 2);
        assert_eq!(index.entries.len(), 2);
        // Aucun dossier temporaire laissé, pas d'écrasement.
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
        assert!(write(&g, out.path(), "2026-11-27 09:00:00", "x", |_| Ok(
            Vec::new()
        ))
        .unwrap_err()
        .contains("existe déjà"));
        // Échec du rendu : rien n'est laissé.
        let out2 = tempfile::tempdir().unwrap();
        assert!(write(&g, out2.path(), "2026-11-27 09:00:00", "x", |_| Err(
            crate::AttestError::Invalid("rendu".into())
        ))
        .is_err());
        assert_eq!(fs::read_dir(out2.path()).unwrap().count(), 0);
    }
}

#[cfg(test)]
mod fixture {
    #![allow(clippy::unwrap_used)]
    use super::tests::write_lot;

    /// Lots d'essai pour l'interface : `SECMET_LOT_FIXTURE=<dossier>
    /// cargo test -p secmet-attest fixture -- --ignored`. Deux « clés »,
    /// un portable repassé de l'une sur l'autre.
    #[test]
    #[ignore = "écrit des lots d'essai dans SECMET_LOT_FIXTURE"]
    fn fixture() {
        let Some(root) = std::env::var_os("SECMET_LOT_FIXTURE") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let disks1: Vec<String> = (0..20).map(|i| format!("PF1A{i:04}")).collect();
        let mut d1: Vec<(&str, bool, &str)> = disks1
            .iter()
            .map(|s| (s.as_str(), true, "2026-11-26 10:00:00"))
            .collect();
        d1[3].1 = false;
        write_lot(&root.join("cle1"), 1, "L-20261126-AB12", "Mission 42", &d1);
        let disks2: Vec<String> = (0..20).map(|i| format!("PF2B{i:04}")).collect();
        let mut d2: Vec<(&str, bool, &str)> = disks2
            .iter()
            .map(|s| (s.as_str(), true, "2026-11-26 15:00:00"))
            .collect();
        d2[0].0 = "PF1A0003";
        write_lot(&root.join("cle2"), 2, "L-20261126-CD34", "Mission 42", &d2);
    }
}
