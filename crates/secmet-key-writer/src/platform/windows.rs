//! Windows : phase 2 (liste par les API de stockage, verrouillage et
//! démontage des volumes, écriture sur `\\.\PhysicalDriveN`). Pour
//! l'instant, tout renvoie une erreur explicite : le programme se compile
//! et affiche l'interface, mais n'écrit pas.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::disks::Disk;

const TODO: &str = "Windows : écriture non encore prise en charge (phase 2)";

pub fn list_disks() -> Result<Vec<Disk>, String> {
    Err(TODO.to_string())
}

pub fn unmount_partitions(_device: &str) -> io::Result<Vec<String>> {
    Err(io::Error::other(TODO))
}

pub fn open_write(_device: &str) -> io::Result<File> {
    Err(io::Error::other(TODO))
}

pub fn open_read(_device: &str) -> io::Result<File> {
    Err(io::Error::other(TODO))
}

pub fn device_size(_f: &File) -> io::Result<u64> {
    Err(io::Error::other(TODO))
}

pub fn drop_caches(_f: &File) -> io::Result<()> {
    Ok(())
}

pub fn reread_partitions(_f: &File) -> io::Result<()> {
    Ok(())
}

pub fn is_privileged() -> bool {
    false
}

pub fn elevated_worker(device: &str, image: &Path) -> io::Result<(Command, Option<PathBuf>)> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--worker").arg(device).arg(image);
    Ok((cmd, None))
}

pub fn elevation_hint(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("le processus d'écriture s'est arrêté (code {c})"),
        None => "le processus d'écriture a été tué".to_string(),
    }
}
