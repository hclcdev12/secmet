//! Tampons alignés sur une page, zéro-initialisés, effacés à la libération.
//!
//! Requis par `O_DIRECT` et par les transferts DMA des commandes pass-through.
//! Le contenu est écrasé (`zeroize`) avant libération : un tampon peut avoir
//! porté un mot de passe ATA ou des secteurs lus.

use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::ptr::NonNull;

use zeroize::Zeroize;

/// Alignement de tout tampon : une page, multiple de toute taille de bloc logique.
pub const ALIGNMENT: usize = 4096;

/// Tampon tas aligné sur [`ALIGNMENT`], zéroïsé à la création et à la libération.
pub struct AlignedBuffer {
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: le tampon possède exclusivement son allocation, comme un `Vec<u8>` ;
// aucun partage interne, donc l'envoyer à un autre thread est sûr.
unsafe impl Send for AlignedBuffer {}

impl AlignedBuffer {
    /// Tampon zéroïsé de `len` octets (au moins 1 octet alloué).
    pub fn new(len: usize) -> Self {
        let layout = Self::layout(len);
        // SAFETY: `layout` a une taille non nulle (au moins 1) et un alignement
        // puissance de deux ; `alloc_zeroed` initialise tous les octets.
        let raw = unsafe { alloc_zeroed(layout) };
        let Some(ptr) = NonNull::new(raw) else {
            handle_alloc_error(layout);
        };
        Self { ptr, len }
    }

    fn layout(len: usize) -> Layout {
        match Layout::from_size_align(len.max(1), ALIGNMENT) {
            Ok(layout) => layout,
            // Seulement atteignable pour des tailles proches de isize::MAX.
            Err(_) => handle_alloc_error(Layout::new::<u8>()),
        }
    }

    /// Longueur utile en octets.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Vrai si la longueur utile est nulle.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Pointeur brut pour le noyau (valide `len` octets).
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Efface le contenu immédiatement (sans attendre la libération).
    pub fn wipe(&mut self) {
        self[..].zeroize();
    }
}

impl std::ops::Deref for AlignedBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: `ptr` pointe `len` octets initialisés (zéro à l'allocation,
        // ou écrits depuis) possédés par ce tampon pour toute sa durée de vie.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl std::ops::DerefMut for AlignedBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: comme `deref`, et `&mut self` garantit l'exclusivité.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        self.wipe();
        // SAFETY: `ptr` a été alloué par `alloc_zeroed` avec exactement ce
        // layout et n'est libéré qu'une fois, ici.
        unsafe { dealloc(self.ptr.as_ptr(), Self::layout(self.len)) }
    }
}

impl std::fmt::Debug for AlignedBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AlignedBuffer({} octets @ {:p})", self.len, self.ptr)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn buffers_are_aligned_zeroed_and_writable() {
        for len in [1, 512, 4096, 1 << 20] {
            let mut buffer = AlignedBuffer::new(len);
            assert_eq!(buffer.as_mut_ptr() as usize % ALIGNMENT, 0);
            assert_eq!(buffer.len(), len);
            assert!(buffer.iter().all(|&b| b == 0));
            buffer[len - 1] = 0xab;
            assert_eq!(buffer[len - 1], 0xab);
            buffer.wipe();
            assert_eq!(buffer[len - 1], 0);
        }
    }

    #[test]
    fn zero_length_is_allowed() {
        let buffer = AlignedBuffer::new(0);
        assert!(buffer.is_empty());
        assert_eq!(&buffer[..], &[]);
    }
}
