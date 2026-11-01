//! Append-friendly MWEB coin changeset for parallel persistence.
//!
//! Enabled with feature `persist`. Compatible with `bdk_file_store::Store`.

use alloc::collections::{BTreeMap, BTreeSet};

use bdk_core::Merge;
use serde::{Deserialize, Serialize};

use crate::coin_db::MwebCoin;

/// Delta applied to an [`crate::MwebCoinDatabase`].
///
/// - `coins`: upsert unspent entries by `output_id`
/// - `spent`: move those ids from unspent → spent (value is the coin at spend time)
///
/// - `restored`: ids whose earlier spend is undone (the spend never confirmed)
///
/// On merge conflict for the same id, `spent` wins over `coins` unless the
/// newer changeset lists the id in `restored`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Upsert unspent coins by output id.
    #[serde(default)]
    pub coins: BTreeMap<[u8; 32], MwebCoin>,
    /// Spent coin snapshots by output id.
    #[serde(default)]
    pub spent: BTreeMap<[u8; 32], MwebCoin>,
    /// Coins moved back from spent to unspent; their snapshot is in `coins`.
    #[serde(default)]
    pub restored: BTreeSet<[u8; 32]>,
}

impl Merge for ChangeSet {
    fn merge(&mut self, other: Self) {
        for id in &other.restored {
            self.spent.remove(id);
        }
        for (id, coin) in other.coins {
            if self.spent.contains_key(&id) {
                // Already spent in self; ignore a later unspent upsert for the same id.
                continue;
            }
            self.coins.insert(id, coin);
        }
        for (id, coin) in other.spent {
            self.coins.remove(&id);
            self.spent.insert(id, coin);
        }
        self.restored.extend(other.restored);
        let spent = &self.spent;
        self.restored.retain(|id| !spent.contains_key(id));
    }

    fn is_empty(&self) -> bool {
        self.coins.is_empty() && self.spent.is_empty() && self.restored.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coin(id_byte: u8, amount: u64) -> MwebCoin {
        MwebCoin {
            output_id: [id_byte; 32],
            commitment: [1; 33],
            amount,
            address_index: 0,
            blind: [2; 32],
            shared_secret: [3; 32],
            spend_key: Some([4; 32]),
            block_height: None,
            is_pegin: false,
            leaf_index: None,
        }
    }

    #[test]
    fn merge_spent_wins_over_coins() {
        let mut a = ChangeSet::default();
        a.coins.insert([1; 32], coin(1, 100));

        let mut b = ChangeSet::default();
        b.spent.insert([1; 32], coin(1, 100));

        a.merge(b);
        assert!(a.coins.is_empty());
        assert!(a.spent.contains_key(&[1; 32]));
    }

    #[test]
    fn merge_ignores_coin_after_spent_in_self() {
        let mut a = ChangeSet::default();
        a.spent.insert([1; 32], coin(1, 100));

        let mut b = ChangeSet::default();
        b.coins.insert([1; 32], coin(1, 200));

        a.merge(b);
        assert!(a.coins.is_empty());
        assert_eq!(a.spent[&[1; 32]].amount, 100);
    }

    #[test]
    fn merge_restored_undoes_an_earlier_spend() {
        let mut a = ChangeSet::default();
        a.spent.insert([1; 32], coin(1, 100));

        let mut b = ChangeSet::default();
        b.coins.insert([1; 32], coin(1, 100));
        b.restored.insert([1; 32]);

        a.merge(b);
        assert!(a.spent.is_empty());
        assert_eq!(a.coins[&[1; 32]].amount, 100);

        let mut c = ChangeSet::default();
        c.spent.insert([1; 32], coin(1, 100));
        a.merge(c);
        assert!(a.coins.is_empty());
        assert!(
            a.restored.is_empty(),
            "a later spend supersedes the restore"
        );
    }
}
