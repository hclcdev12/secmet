//! ATA Security feature set : mots de passe et bloc de données des commandes
//! SECURITY (ACS-4 §7.42 à 7.47).
//!
//! Le mot de passe est un type dédié, effacé à la libération et jamais
//! affiché. SecMet utilisait un mot de passe constant « PASS » avec hdparm ;
//! [`Password::from_ascii`] le reproduit, [`Password::random`] en propose un
//! aléatoire (à enregistrer durablement avant de le poser).

use std::io::Read;

use zeroize::Zeroizing;

use crate::{DeviceError, Result};

/// Bit 0 du mot de contrôle : identifiant Master (sinon User).
pub const MASTER_PASSWORD: u16 = 1 << 0;
/// Bit 1 du mot de contrôle de SECURITY ERASE UNIT : enhanced erase.
pub const ENHANCED_ERASE: u16 = 1 << 1;
/// Bit 8 du mot de contrôle de SET PASSWORD : niveau Maximum. Jamais posé par
/// SecMet (niveau High : le master password constructeur reste utilisable).
pub const LEVEL_MAXIMUM: u16 = 1 << 8;

/// Mot de passe Security : 32 octets, bourrés de zéros, effacés à la libération.
#[derive(Clone)]
pub struct Password(Zeroizing<[u8; 32]>);

impl Password {
    /// Mot de passe à partir d'un texte ASCII de 1 à 32 octets, complété par
    /// des zéros (convention hdparm, donc interopérable avec les outils standard).
    pub fn from_ascii(text: &str) -> Result<Self> {
        let bytes = text.as_bytes();
        if bytes.is_empty() || bytes.len() > 32 || !text.is_ascii() {
            return Err(DeviceError::InvalidArgument(
                "mot de passe ATA : 1 à 32 caractères ASCII".to_string(),
            ));
        }
        let mut password = Zeroizing::new([0u8; 32]);
        password[..bytes.len()].copy_from_slice(bytes);
        Ok(Self(password))
    }

    /// Mot de passe depuis 32 octets exacts (relecture d'un enregistrement).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Mot de passe aléatoire de 32 caractères alphanumériques, lu sur
    /// `/dev/urandom` avec échantillonnage par rejet (distribution uniforme).
    pub fn random() -> Result<Self> {
        const ALPHABET: &[u8; 62] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut urandom =
            std::fs::File::open("/dev/urandom").map_err(|e| DeviceError::io("/dev/urandom", e))?;
        let mut password = Zeroizing::new([0u8; 32]);
        let mut filled = 0;
        while filled < 32 {
            let mut random = Zeroizing::new([0u8; 64]);
            urandom
                .read_exact(random.as_mut())
                .map_err(|e| DeviceError::io("lecture /dev/urandom", e))?;
            // 248 = 4 × 62 : les octets ≥ 248 sont rejetés pour rester uniforme.
            for &byte in random.iter().filter(|&&b| b < 248) {
                if filled == 32 {
                    break;
                }
                password[filled] = ALPHABET[usize::from(byte % 62)];
                filled += 1;
            }
        }
        Ok(Self(password))
    }

    /// Les 32 octets, pour le disque et un enregistrement de secours uniquement.
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(<masqué>)")
    }
}

/// À qui appartient le mot de passe d'une commande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identifier {
    /// Mot de passe utilisateur.
    User,
    /// Mot de passe maître.
    Master,
}

impl Identifier {
    /// Bits du mot de contrôle.
    pub fn control_bits(self) -> u16 {
        match self {
            Self::User => 0,
            Self::Master => MASTER_PASSWORD,
        }
    }
}

/// Remplit le bloc de 512 octets d'une commande Security : mot de contrôle,
/// puis le mot de passe tel quel à partir de l'octet 2. Écrit dans un tampon
/// fourni par l'appelant (qui l'efface) plutôt que renvoyé par valeur.
pub fn fill_password_block(block: &mut [u8; 512], control: u16, password: &Password) {
    block.fill(0);
    block[..2].copy_from_slice(&control.to_le_bytes());
    block[2..34].copy_from_slice(password.bytes());
}

/// Délai minimal pour SECURITY ERASE UNIT, quelle que soit l'estimation.
const MIN_TIMEOUT_MINUTES: u32 = 60;

/// Délai de SECURITY ERASE UNIT : deux fois l'estimation du disque, une
/// heure au moins, en millisecondes.
pub fn erase_timeout_ms(estimate_minutes: Option<u32>) -> u32 {
    estimate_minutes
        .map_or(MIN_TIMEOUT_MINUTES, |m| m.saturating_mul(2))
        .max(MIN_TIMEOUT_MINUTES)
        .saturating_mul(60_000)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn ascii_password_is_zero_padded_like_hdparm() {
        let p = Password::from_ascii("PASS").expect("court");
        assert_eq!(&p.bytes()[..4], b"PASS");
        assert!(p.bytes()[4..].iter().all(|&b| b == 0));
        assert!(Password::from_ascii("").is_err());
        assert!(Password::from_ascii(&"x".repeat(33)).is_err());
        assert!(Password::from_ascii("é").is_err());
        assert_eq!(format!("{p:?}"), "Password(<masqué>)");
    }

    #[test]
    fn random_passwords_are_alphanumeric_and_distinct() {
        let a = Password::random().expect("urandom");
        let b = Password::random().expect("urandom");
        assert!(a.bytes().iter().all(u8::is_ascii_alphanumeric));
        assert_ne!(a.bytes(), b.bytes());
    }

    #[test]
    fn password_block_layout() {
        let password = Password::from_bytes(*b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345");
        let mut block = [0xffu8; 512];
        fill_password_block(&mut block, ENHANCED_ERASE, &password);
        assert_eq!(&block[..2], &[0x02, 0x00]);
        assert_eq!(&block[2..34], password.bytes());
        assert!(block[34..].iter().all(|&b| b == 0));
        assert_eq!(Identifier::Master.control_bits(), MASTER_PASSWORD);
        assert_eq!(LEVEL_MAXIMUM, 0x100);
    }

    #[test]
    fn timeout_is_twice_the_estimate_and_at_least_an_hour() {
        assert_eq!(erase_timeout_ms(None), 60 * 60_000);
        assert_eq!(erase_timeout_ms(Some(10)), 60 * 60_000);
        assert_eq!(erase_timeout_ms(Some(300)), 600 * 60_000);
    }
}
