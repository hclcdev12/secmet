//! Windows : disques `\\.\PhysicalDriveN` interrogés par `DeviceIoControl`
//! (propriétés du périphérique, géométrie, volumes et leurs lettres),
//! verrouillage et démontage de chaque volume de la clé
//! (`FSCTL_LOCK_VOLUME`, `FSCTL_DISMOUNT_VOLUME`, poignées gardées ouvertes
//! jusqu'à la fin), écriture directe sans cache
//! (`FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`), puis
//! `IOCTL_DISK_UPDATE_PROPERTIES`. Les droits d'administrateur sont pris
//! au lancement de l'interface (relance de l'exécutable par `ShellExecuteEx`
//! avec le verbe `runas`, invite UAC) ; le processus `--worker` en hérite.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_QUERY};
use windows_sys::Win32::Storage::FileSystem::{
    BusTypeAta, BusTypeMmc, BusTypeNvme, BusTypeSas, BusTypeSata, BusTypeScsi, BusTypeSd,
    BusTypeUsb, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose,
    GetVolumePathNamesForVolumeNameW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_WRITE_THROUGH,
    FILE_SHARE_READ, FILE_SHARE_WRITE, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
    IOCTL_DISK_GET_LENGTH_INFO, IOCTL_DISK_UPDATE_PROPERTIES, IOCTL_STORAGE_QUERY_PROPERTY,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcessToken, CREATE_NO_WINDOW,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONASTERISK, MB_ICONERROR, MB_ICONHAND, MB_OK, SW_SHOWNORMAL,
};

use crate::disks::Disk;

/// Volumes de la clé verrouillés et démontés par [`unmount_partitions`] ;
/// les poignées restent ouvertes (Windows ne remonte rien) jusqu'à
/// [`reread_partitions`] ou la fin du processus.
static LOCKED: Mutex<Vec<File>> = Mutex::new(Vec::new());

/// Chaîne UTF-16 terminée par zéro.
fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap_or([0; 4]))
}

fn i64_at(b: &[u8], off: usize) -> i64 {
    i64::from_le_bytes(b[off..off + 8].try_into().unwrap_or([0; 8]))
}

/// Chaîne ASCII terminée par zéro à l'offset donné d'une structure.
fn str_at(b: &[u8], off: u32) -> Option<String> {
    let off = off as usize;
    if off == 0 || off >= b.len() {
        return None;
    }
    let end = b[off..]
        .iter()
        .position(|&c| c == 0)
        .map_or(b.len(), |p| off + p);
    let s = String::from_utf8_lossy(&b[off..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Vrai pour un chemin de périphérique (`\\.\…`, `\\?\…`), faux pour un
/// fichier ordinaire (essais).
fn is_device(path: &str) -> bool {
    path.starts_with(r"\\.\") || path.starts_with(r"\\?\")
}

/// `DeviceIoControl` : entrée optionnelle, sortie dans `out`, nombre
/// d'octets renvoyés.
fn ioctl(f: &File, code: u32, input: &[u8], out: &mut [u8]) -> io::Result<usize> {
    let mut returned: u32 = 0;
    // SAFETY: poignée valide ; tampons valides pour les longueurs passées ;
    // aucun pointeur conservé après l'appel.
    let ok = unsafe {
        DeviceIoControl(
            f.as_raw_handle(),
            code,
            if input.is_empty() {
                std::ptr::null()
            } else {
                input.as_ptr().cast()
            },
            input.len() as u32,
            if out.is_empty() {
                std::ptr::null_mut()
            } else {
                out.as_mut_ptr().cast()
            },
            out.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(returned as usize)
}

/// Ouvre un périphérique (accès `access`, partage lecture + écriture).
fn open_device(path: &str, access: u32, flags: u32) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(flags)
        .open(path)
}

/// Volumes présents : nom `\\?\Volume{…}\`, numéros de disque portant
/// leurs extents, chemins d'accès (`E:\`, dossiers de montage).
struct Volume {
    name: String,
    disks: Vec<u32>,
    paths: Vec<String>,
}

fn volumes() -> Vec<Volume> {
    let mut out = Vec::new();
    let mut buf = [0u16; 512];
    // SAFETY: tampon de la taille annoncée ; poignée fermée ci-dessous.
    let h = unsafe { FindFirstVolumeW(buf.as_mut_ptr(), buf.len() as u32) };
    if h == INVALID_HANDLE_VALUE {
        return out;
    }
    loop {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let name = String::from_utf16_lossy(&buf[..len]);
        out.push(Volume {
            disks: volume_disks(&name),
            paths: volume_paths(&name),
            name,
        });
        // SAFETY: même tampon, poignée d'énumération valide.
        if unsafe { FindNextVolumeW(h, buf.as_mut_ptr(), buf.len() as u32) } == 0 {
            break;
        }
    }
    // SAFETY: poignée renvoyée par FindFirstVolumeW.
    unsafe { FindVolumeClose(h) };
    out
}

/// Chemin ouvrable d'un volume : sans la barre finale.
fn volume_device(name: &str) -> String {
    name.trim_end_matches('\\').to_string()
}

fn volume_disks(name: &str) -> Vec<u32> {
    let Ok(f) = open_device(&volume_device(name), 0, 0) else {
        return Vec::new();
    };
    // VOLUME_DISK_EXTENTS : u32 NumberOfDiskExtents, 4 octets de
    // remplissage, puis DISK_EXTENT { u32 DiskNumber ; pad ; i64 ; i64 }
    // (24 octets chacun).
    let mut out = vec![0u8; 8 + 24 * 64];
    let Ok(n) = ioctl(&f, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &[], &mut out) else {
        return Vec::new();
    };
    let count = u32_at(&out, 0) as usize;
    (0..count)
        .filter(|i| 8 + 24 * (i + 1) <= n)
        .map(|i| u32_at(&out, 8 + 24 * i))
        .collect()
}

fn volume_paths(name: &str) -> Vec<String> {
    let wname = wide(name);
    let mut buf = vec![0u16; 4096];
    let mut len: u32 = 0;
    // SAFETY: chaîne terminée par zéro, tampon de la taille annoncée.
    let ok = unsafe {
        GetVolumePathNamesForVolumeNameW(
            wname.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut len,
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    // Liste de chaînes terminées par zéro, close par un zéro de plus.
    buf.truncate((len as usize).min(buf.len()));
    buf.split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

fn bus_name(bus: i32) -> Option<&'static str> {
    Some(match bus {
        b if b == BusTypeUsb => "usb",
        b if b == BusTypeSata => "sata",
        b if b == BusTypeAta => "ata",
        b if b == BusTypeNvme => "nvme",
        b if b == BusTypeSd => "sd",
        b if b == BusTypeMmc => "mmc",
        b if b == BusTypeSas => "sas",
        b if b == BusTypeScsi => "scsi",
        _ => return None,
    })
}

/// Disques physiques `\\.\PhysicalDrive0` à 63 (la numérotation peut
/// avoir des trous), sans droits particuliers (ouverture sans accès,
/// requêtes de propriétés seulement).
pub fn list_disks() -> Result<Vec<Disk>, String> {
    let vols = volumes();
    let mut disks = Vec::new();
    let mut misses = 0;
    for n in 0u32..64 {
        let path = format!(r"\\.\PhysicalDrive{n}");
        let Ok(f) = open_device(&path, 0, 0) else {
            misses += 1;
            if misses > 8 && !disks.is_empty() {
                break;
            }
            continue;
        };
        misses = 0;
        // STORAGE_PROPERTY_QUERY { StorageDeviceProperty = 0,
        // PropertyStandardQuery = 0, AdditionalParameters[1] }.
        let query = [0u8; 12];
        let mut desc = vec![0u8; 1024];
        let Ok(_) = ioctl(&f, IOCTL_STORAGE_QUERY_PROPERTY, &query, &mut desc) else {
            continue;
        };
        // STORAGE_DEVICE_DESCRIPTOR : RemovableMedia @10, VendorIdOffset
        // @12, ProductIdOffset @16, SerialNumberOffset @24, BusType @28.
        let removable_media = desc[10] != 0;
        let vendor = str_at(&desc, u32_at(&desc, 12)).filter(|v| v != "ATA");
        let product = str_at(&desc, u32_at(&desc, 16));
        let serial = str_at(&desc, u32_at(&desc, 24));
        let bus = u32_at(&desc, 28) as i32;
        // DISK_GEOMETRY_EX : DISK_GEOMETRY (24 octets) puis DiskSize i64.
        let mut geo = [0u8; 256];
        let size = match ioctl(&f, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut geo) {
            Ok(_) => i64_at(&geo, 24).max(0) as u64,
            Err(_) => 0,
        };
        let model = product.unwrap_or_else(|| "Disque".to_string());
        let name = match vendor {
            Some(v) if !model.to_lowercase().starts_with(&v.to_lowercase()) => {
                format!("{v} {model}")
            }
            _ => model,
        };
        let mounted: Vec<String> = vols
            .iter()
            .filter(|v| v.disks.contains(&n))
            .flat_map(|v| {
                if v.paths.is_empty() {
                    vec!["(sans lettre)".to_string()]
                } else {
                    v.paths.clone()
                }
            })
            .collect();
        disks.push(Disk {
            path,
            name,
            serial,
            size,
            // Pas le bit « hotplug » : les ports SATA internes le portent
            // souvent ; seuls l'USB, les cartes et les médias amovibles.
            removable: removable_media
                || bus == BusTypeUsb
                || bus == BusTypeSd
                || bus == BusTypeMmc,
            transport: bus_name(bus).map(str::to_string),
            mounted,
        });
    }
    Ok(disks)
}

fn disk_number(device: &str) -> Option<u32> {
    device
        .rsplit("PhysicalDrive")
        .next()
        .and_then(|n| n.parse().ok())
}

/// Verrouille et démonte chaque volume du disque ; les poignées restent
/// ouvertes jusqu'à [`reread_partitions`]. Un volume qu'un autre programme
/// tient (Explorateur ouvert sur la clé, antivirus) fait échouer le
/// verrouillage après quelques essais.
pub fn unmount_partitions(device: &str) -> io::Result<Vec<String>> {
    if !is_device(device) {
        return Ok(Vec::new());
    }
    let Some(n) = disk_number(device) else {
        return Err(io::Error::other(format!(
            "{device} : numéro de disque illisible"
        )));
    };
    let mut done = Vec::new();
    let mut held = Vec::new();
    for v in volumes().into_iter().filter(|v| v.disks.contains(&n)) {
        let label = if v.paths.is_empty() {
            v.name.clone()
        } else {
            v.paths.join(", ")
        };
        let f = open_device(&volume_device(&v.name), GENERIC_READ | GENERIC_WRITE, 0)
            .map_err(|e| io::Error::new(e.kind(), format!("ouverture du volume {label} : {e}")))?;
        let mut last = None;
        for attempt in 0..10 {
            match ioctl(&f, FSCTL_LOCK_VOLUME, &[], &mut []) {
                Ok(_) => {
                    last = None;
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    if attempt < 9 {
                        std::thread::sleep(Duration::from_millis(300));
                    }
                }
            }
        }
        if let Some(e) = last {
            return Err(io::Error::new(
                e.kind(),
                format!(
                    "volume {label} utilisé par un autre programme (fermer les fenêtres ouvertes sur la clé) : {e}"
                ),
            ));
        }
        ioctl(&f, FSCTL_DISMOUNT_VOLUME, &[], &mut [])
            .map_err(|e| io::Error::new(e.kind(), format!("démontage du volume {label} : {e}")))?;
        done.push(label);
        held.push(f);
    }
    if let Ok(mut g) = LOCKED.lock() {
        g.extend(held);
    }
    Ok(done)
}

/// Ouvre le disque en écriture directe : chaque écriture attend la clé
/// (progression réelle, annulation possible, pas de cache à vider à la
/// fin). Tampons et tailles alignés sur [`crate::worker::ALIGN`].
pub fn open_write(device: &str) -> io::Result<File> {
    if !is_device(device) {
        return OpenOptions::new().write(true).open(device);
    }
    open_device(
        device,
        GENERIC_READ | GENERIC_WRITE,
        FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH,
    )
    .map_err(|e| match e.raw_os_error() {
        Some(5) => io::Error::new(
            e.kind(),
            "accès refusé : l'outil doit être lancé en administrateur",
        ),
        Some(32) | Some(33) => io::Error::new(e.kind(), "disque utilisé par un autre programme"),
        _ => e,
    })
}

/// Ouvre le disque en lecture directe (sans cache) pour la relecture.
pub fn open_read(device: &str) -> io::Result<File> {
    if !is_device(device) {
        return File::open(device);
    }
    open_device(device, GENERIC_READ, FILE_FLAG_NO_BUFFERING)
}

/// Taille du disque en octets (`IOCTL_DISK_GET_LENGTH_INFO`).
pub fn device_size(f: &File) -> io::Result<u64> {
    let mut out = [0u8; 8];
    ioctl(f, IOCTL_DISK_GET_LENGTH_INFO, &[], &mut out)?;
    Ok(i64_at(&out, 0).max(0) as u64)
}

/// Rien à vider : écriture et relecture se font sans cache.
pub fn drop_caches(_f: &File) -> io::Result<()> {
    Ok(())
}

/// Libère les volumes verrouillés puis fait relire la table de partitions.
pub fn reread_partitions(f: &File) -> io::Result<()> {
    if let Ok(mut g) = LOCKED.lock() {
        g.clear();
    }
    ioctl(f, IOCTL_DISK_UPDATE_PROPERTIES, &[], &mut [])?;
    Ok(())
}

/// Vrai si le processus a un jeton élevé (administrateur, après UAC).
pub fn is_privileged() -> bool {
    let mut token = std::ptr::null_mut();
    // SAFETY: pseudo-poignée du processus courant ; `token` reçoit une
    // poignée fermée ci-dessous.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut elevation: u32 = 0;
    let mut len: u32 = 0;
    // SAFETY: TOKEN_ELEVATION est un u32 ; tampon et taille concordent.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut u32).cast(),
            4,
            &mut len,
        )
    };
    // SAFETY: poignée ouverte ci-dessus.
    unsafe { CloseHandle(token) };
    ok != 0 && elevation != 0
}

/// Processus d'écriture : le même exécutable, qui hérite des droits de
/// l'interface (déjà élevée), sans fenêtre de console.
pub fn elevated_worker(device: &str, image: &Path) -> io::Result<(Command, Option<PathBuf>)> {
    if !is_privileged() {
        return Err(io::Error::other(
            "droits d'administrateur requis : relancer l'outil en administrateur",
        ));
    }
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--worker")
        .arg(device)
        .arg(image)
        .creation_flags(CREATE_NO_WINDOW);
    Ok((cmd, None))
}

/// Message quand le processus d'écriture s'arrête sans résultat.
pub fn elevation_hint(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("le processus d'écriture s'est arrêté (code {c})"),
        None => "le processus d'écriture a été tué".to_string(),
    }
}

/// Un argument de ligne de commande entre guillemets, selon les règles de
/// `CommandLineToArgvW`.
fn quote_arg(a: &str) -> String {
    if !a.is_empty() && !a.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
        return a.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in a.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// Relance ce programme en administrateur (invite UAC) avec les mêmes
/// arguments ; l'appelant se termine ensuite. Erreur `ERROR_CANCELLED`
/// (1223) si l'utilisateur refuse.
pub fn relaunch_elevated(args: &[String]) -> io::Result<()> {
    let exe = wide(&std::env::current_exe()?.to_string_lossy());
    let params = wide(
        &args
            .iter()
            .map(|a| quote_arg(a))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let dir = wide(
        &std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    let verb = wide("runas");
    // SAFETY: structure entièrement initialisée (zéros puis champs) ; les
    // chaînes vivent jusqu'au retour de l'appel.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = exe.as_ptr();
    info.lpParameters = params.as_ptr();
    info.lpDirectory = dir.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: pointeur vers la structure ci-dessus.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !info.hProcess.is_null() {
        // SAFETY: poignée renvoyée avec SEE_MASK_NOCLOSEPROCESS.
        unsafe { CloseHandle(info.hProcess) };
    }
    Ok(())
}

/// Boîte de message d'erreur (avant que l'interface existe).
pub fn message_box(title: &str, text: &str) {
    let t = wide(title);
    let x = wide(text);
    // SAFETY: chaînes terminées par zéro, sans fenêtre parente.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            x.as_ptr(),
            t.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

/// Son système de fin (succès ou échec), en plus de la fenêtre.
pub fn chime(ok: bool) {
    // SAFETY: appel sans pointeur.
    unsafe { MessageBeep(if ok { MB_ICONASTERISK } else { MB_ICONHAND }) };
}

/// Pour `--verify`, `--list-disks`, `--version` lancés depuis une console :
/// l'exécutable est sans console (sous-système Windows) ; il s'attache à
/// celle du parent quand sa sortie n'est pas déjà redirigée.
pub fn attach_console() {
    // SAFETY: appels sans pointeur ; échec sans conséquence.
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
