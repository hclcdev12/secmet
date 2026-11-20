//! Opérations en cours dans l'interface, et blocage de l'installation d'une
//! mise à jour (Annexe N, §N.5 ; décision D14 : bloquée partout).
//!
//! Chaque opération longue (effacement, analyse des disques, TestDisk,
//! chargement de l'inventaire, PDF, clôture de lot, déverrouillage,
//! dé-frozen) prend une garde [`begin`] qu'elle garde jusqu'à sa fin ; la
//! garde se libère d'elle-même quand elle est abandonnée (fin du minuteur
//! qui la porte, retour anticipé) : un oubli ne peut pas bloquer le bouton
//! pour toujours. Les Réglages relisent [`current`] en continu et
//! demandent à [`install_blocker`], fonction pure, si l'installation est
//! permise et sinon pourquoi.
//!
//! Module sans GTK : compteurs propres au fil de l'interface
//! (`thread_local`), la garde n'est pas `Send`.

use std::cell::Cell;
use std::marker::PhantomData;

/// Opération longue qui interdit d'installer une mise à jour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusyKind {
    /// Effacement (session d'effacement en cours).
    Erase,
    /// Analyse des disques (scan par disque, SMART, protections).
    Smart,
    /// Vérification TestDisk.
    TestDisk,
    /// Chargement de l'inventaire des disques.
    Inventory,
    /// Génération d'un certificat PDF.
    Pdf,
    /// Clôture d'un lot (PDF groupé, manifeste, copie, archivage).
    LotClose,
    /// Déverrouillage ATA Security.
    Unlock,
    /// Sortie de l'état gelé (FROZEN) par mise en veille.
    Defrozen,
}

impl BusyKind {
    /// Toutes les opérations, dans l'ordre où la raison du blocage est
    /// choisie (la plus grave d'abord).
    pub const ALL: [BusyKind; 8] = [
        BusyKind::Erase,
        BusyKind::LotClose,
        BusyKind::Unlock,
        BusyKind::Defrozen,
        BusyKind::TestDisk,
        BusyKind::Pdf,
        BusyKind::Smart,
        BusyKind::Inventory,
    ];

    fn index(self) -> usize {
        match self {
            BusyKind::Erase => 0,
            BusyKind::LotClose => 1,
            BusyKind::Unlock => 2,
            BusyKind::Defrozen => 3,
            BusyKind::TestDisk => 4,
            BusyKind::Pdf => 5,
            BusyKind::Smart => 6,
            BusyKind::Inventory => 7,
        }
    }
}

thread_local! {
    static COUNTS: [Cell<u32>; 8] = const { [const { Cell::new(0) }; 8] };
    static RESTART_PENDING: Cell<bool> = const { Cell::new(false) };
    static INSTALLING: Cell<u32> = const { Cell::new(0) };
}

/// Garde d'une opération en cours : l'opération compte tant que la garde
/// vit. Liée au fil qui l'a prise (pas `Send`).
#[derive(Debug)]
#[must_use = "l'opération ne compte que tant que la garde est gardée"]
pub struct BusyGuard {
    kind: BusyKind,
    _not_send: PhantomData<*const ()>,
}

impl BusyGuard {
    /// Opération gardée.
    pub fn kind(&self) -> BusyKind {
        self.kind
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let i = self.kind.index();
        COUNTS.with(|c| c[i].set(c[i].get().saturating_sub(1)));
    }
}

/// Début d'une opération `kind` ; elle compte jusqu'à la libération de la
/// garde rendue.
pub fn begin(kind: BusyKind) -> BusyGuard {
    let i = kind.index();
    COUNTS.with(|c| c[i].set(c[i].get().saturating_add(1)));
    BusyGuard {
        kind,
        _not_send: PhantomData,
    }
}

/// Opérations en cours (sans doublon), dans l'ordre de [`BusyKind::ALL`].
pub fn current() -> Vec<BusyKind> {
    COUNTS.with(|c| {
        BusyKind::ALL
            .iter()
            .copied()
            .filter(|k| c[k.index()].get() > 0)
            .collect()
    })
}

/// Garde de l'installation d'une mise à jour en cours (téléchargement,
/// vérification, gestionnaire de paquets) : ni les Réglages ni la fenêtre
/// principale ne se ferment tant qu'elle vit.
#[derive(Debug)]
#[must_use = "l'installation ne compte que tant que la garde est gardée"]
pub struct InstallGuard {
    _not_send: PhantomData<*const ()>,
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.with(|i| i.set(i.get().saturating_sub(1)));
    }
}

/// Début de l'installation d'une mise à jour.
pub fn begin_install() -> InstallGuard {
    INSTALLING.with(|i| i.set(i.get().saturating_add(1)));
    InstallGuard {
        _not_send: PhantomData,
    }
}

/// Une installation de mise à jour est-elle en cours ?
pub fn installing() -> bool {
    INSTALLING.with(|i| i.get() > 0)
}

/// Une mise à jour vient d'être installée : l'ancien binaire tourne encore
/// jusqu'à la relance. Aucun lot ne doit être ouvert sous lui.
pub fn set_restart_pending() {
    RESTART_PENDING.with(|r| r.set(true));
}

/// Relance attendue après une installation réussie.
pub fn restart_pending() -> bool {
    RESTART_PENDING.with(|r| r.get())
}

/// Raison pour laquelle l'installation d'une mise à jour est refusée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    /// Une installation est déjà en cours.
    Installing,
    /// Une version a été installée : relancer SecMet d'abord.
    RestartPending,
    /// Une opération longue est en cours.
    Busy(BusyKind),
    /// Un lot est ouvert (son identifiant) : le clôturer d'abord, un lot
    /// ne mélange pas deux versions du logiciel.
    OpenLot(String),
}

/// Décide si l'installation d'une mise à jour est permise : `None` si
/// oui, sinon la raison du refus. Pure : `busy` vient de [`current`],
/// `open_lot` du lot actif de l'interface, `installing` de la fenêtre des
/// Réglages. Une opération en cours prime sur le lot ouvert (elle est
/// passagère, la raison affichée change d'elle-même à sa fin).
pub fn install_blocker(
    busy: &[BusyKind],
    open_lot: Option<&str>,
    installing: bool,
    restart_pending: bool,
) -> Option<Blocker> {
    if installing {
        return Some(Blocker::Installing);
    }
    if restart_pending {
        return Some(Blocker::RestartPending);
    }
    if let Some(kind) = BusyKind::ALL.iter().find(|k| busy.contains(k)) {
        return Some(Blocker::Busy(*kind));
    }
    open_lot.map(|id| Blocker::OpenLot(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_running_allows_installation() {
        assert_eq!(install_blocker(&[], None, false, false), None);
    }

    #[test]
    fn every_operation_blocks() {
        for kind in BusyKind::ALL {
            assert_eq!(
                install_blocker(&[kind], None, false, false),
                Some(Blocker::Busy(kind))
            );
            assert_eq!(
                install_blocker(&[kind], Some("L-20261120-ABCD"), false, false),
                Some(Blocker::Busy(kind)),
                "une opération prime sur le lot ouvert"
            );
        }
    }

    #[test]
    fn open_lot_blocks() {
        assert_eq!(
            install_blocker(&[], Some("L-20261120-ABCD"), false, false),
            Some(Blocker::OpenLot("L-20261120-ABCD".into()))
        );
    }

    #[test]
    fn installing_and_restart_take_precedence() {
        let all = BusyKind::ALL;
        assert_eq!(
            install_blocker(&all, Some("L-1"), true, true),
            Some(Blocker::Installing)
        );
        assert_eq!(
            install_blocker(&all, Some("L-1"), false, true),
            Some(Blocker::RestartPending)
        );
        assert_eq!(
            install_blocker(&[], None, false, true),
            Some(Blocker::RestartPending)
        );
        assert_eq!(
            install_blocker(&[], None, true, false),
            Some(Blocker::Installing)
        );
    }

    #[test]
    fn the_most_serious_operation_is_reported() {
        assert_eq!(
            install_blocker(
                &[BusyKind::Inventory, BusyKind::Smart, BusyKind::Erase],
                None,
                false,
                false
            ),
            Some(Blocker::Busy(BusyKind::Erase))
        );
        assert_eq!(
            install_blocker(&[BusyKind::Smart, BusyKind::TestDisk], None, false, false),
            Some(Blocker::Busy(BusyKind::TestDisk))
        );
    }

    #[test]
    fn guards_count_and_release() {
        // Compteurs propres au fil : chaque test a les siens.
        assert!(current().is_empty());
        let a = begin(BusyKind::Smart);
        let b = begin(BusyKind::Smart);
        let c = begin(BusyKind::Erase);
        assert_eq!(a.kind(), BusyKind::Smart);
        assert_eq!(current(), vec![BusyKind::Erase, BusyKind::Smart]);
        drop(a);
        assert_eq!(current(), vec![BusyKind::Erase, BusyKind::Smart]);
        drop(b);
        assert_eq!(current(), vec![BusyKind::Erase]);
        drop(c);
        assert!(current().is_empty());
    }

    #[test]
    fn a_guard_moved_into_a_closure_is_released_with_it() {
        let guard = begin(BusyKind::Pdf);
        let closure = move || guard.kind();
        assert_eq!(current(), vec![BusyKind::Pdf]);
        drop(closure);
        assert!(current().is_empty());
    }

    #[test]
    fn install_guard_counts() {
        assert!(!installing());
        let guard = begin_install();
        assert!(installing());
        drop(guard);
        assert!(!installing());
    }

    #[test]
    fn restart_flag_is_sticky() {
        assert!(!restart_pending());
        set_restart_pending();
        assert!(restart_pending());
    }

    #[test]
    fn indices_are_distinct() {
        let mut seen: Vec<usize> = BusyKind::ALL.iter().map(|k| k.index()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), BusyKind::ALL.len());
    }
}
