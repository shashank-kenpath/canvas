//! This device's copy of a team board, kept in step with the board's room.
//!
//! The room orders every change; the last change it receives for a record
//! wins. To agree with the room, a change from someone else is ignored for
//! records this device has changed but the room has not yet confirmed: the
//! room handled the other change first, so ours lands on top of it.
//!
//! Records are opaque JSON objects with a string `id`. The app decides what
//! a record is (a shape on a page, the page order, ...).

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Who made a record and who changed it last.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Attribution {
    pub created_by: String,
    pub edited_by: String,
    /// Milliseconds since the Unix epoch.
    pub edited_at: i64,
}

/// One batch of local changes, ready to send.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub id: u64,
    pub put: Vec<Value>,
    pub del: Vec<String>,
}

impl Batch {
    pub fn to_message(&self) -> Value {
        json!({ "t": "ops", "id": self.id, "put": self.put, "del": self.del })
    }
}

/// Changes to make to the local board.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub put: Vec<Value>,
    pub del: Vec<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.del.is_empty()
    }
}

/// Largest batch, in bytes of JSON, sent in one message.
const BATCH_BYTES: usize = 512 << 10;

#[derive(Default)]
pub struct Replica {
    pub epoch: String,
    pub rev: u64,
    /// Every record as the room has it, as far as this device knows,
    /// including changes this device sent.
    known: HashMap<String, Value>,
    /// Records with unconfirmed changes, and the newest batch that changed them.
    pending: HashMap<String, u64>,
    next_batch: u64,
    pub attribution: HashMap<String, Attribution>,
    /// Records received in a full sync, to find what the room no longer has.
    full_sync: Option<HashSet<String>>,
}

impl Replica {
    pub fn new(epoch: String, rev: u64, known: HashMap<String, Value>, attribution: HashMap<String, Attribution>) -> Self {
        Replica { epoch, rev, known, attribution, ..Default::default() }
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Records with changes the room has not confirmed, for saving.
    pub fn pending_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.pending.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Stops tracking a record this device cannot use, so it is neither
    /// sent nor deleted.
    pub fn forget(&mut self, id: &str) {
        self.known.remove(id);
        self.pending.remove(id);
    }

    pub fn known(&self) -> &HashMap<String, Value> {
        &self.known
    }

    /// Batches that turn what the room has into `now`, the board as it is on
    /// this device. Large changes are split across several batches.
    pub fn local(&mut self, now: &HashMap<String, Value>) -> Vec<Batch> {
        let mut put: Vec<&Value> = now.iter().filter(|(id, v)| self.known.get(*id) != Some(*v)).map(|(_, v)| v).collect();
        let mut del: Vec<String> = self.known.keys().filter(|id| !now.contains_key(*id)).cloned().collect();
        // Stable order keeps batches deterministic for tests and retries.
        put.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        del.sort();
        let mut batches = Vec::new();
        let mut current = Batch { id: 0, put: Vec::new(), del: Vec::new() };
        let mut bytes = 0;
        for v in put {
            let size = v.to_string().len();
            if bytes + size > BATCH_BYTES && !(current.put.is_empty() && current.del.is_empty()) {
                batches.push(std::mem::replace(&mut current, Batch { id: 0, put: Vec::new(), del: Vec::new() }));
                bytes = 0;
            }
            bytes += size;
            current.put.push(v.clone());
        }
        current.del = del;
        if !current.put.is_empty() || !current.del.is_empty() {
            batches.push(current);
        }
        for batch in &mut batches {
            self.next_batch += 1;
            batch.id = self.next_batch;
            for v in &batch.put {
                let id = v["id"].as_str().unwrap_or_default().to_string();
                self.pending.insert(id.clone(), batch.id);
                self.known.insert(id, v.clone());
            }
            for id in &batch.del {
                self.pending.insert(id.clone(), batch.id);
                self.known.remove(id);
            }
        }
        batches
    }

    /// Unconfirmed changes again, as they stand now, after reconnecting.
    pub fn resend(&mut self, now: &HashMap<String, Value>) -> Vec<Batch> {
        let ids: Vec<String> = self.pending.keys().cloned().collect();
        self.pending.clear();
        // Forget what was sent so `local` sends it again.
        for id in ids {
            match now.get(&id) {
                Some(_) => {
                    self.known.remove(&id);
                }
                None => {
                    self.known.insert(id.clone(), json!({ "id": id }));
                }
            }
        }
        self.local(now)
    }

    /// The room confirmed batch `id`.
    pub fn ack(&mut self, id: u64, rev: u64, at: i64, me: &str) {
        let confirmed: Vec<String> = self.pending.iter().filter(|(_, b)| **b == id).map(|(r, _)| r.clone()).collect();
        for record in confirmed {
            self.pending.remove(&record);
            if self.known.contains_key(&record) {
                let a = self.attribution.entry(record).or_default();
                if a.created_by.is_empty() {
                    a.created_by = me.to_string();
                }
                a.edited_by = me.to_string();
                a.edited_at = at;
            } else {
                self.attribution.remove(&record);
            }
        }
        // Batches are confirmed in order, so earlier unconfirmed ones were refused.
        self.pending.retain(|_, b| *b > id);
        self.rev = self.rev.max(rev);
    }

    /// Starts a sync after connecting. A full sync replaces everything.
    pub fn welcome(&mut self, epoch: &str, full: bool) {
        if full || epoch != self.epoch {
            self.full_sync = Some(HashSet::new());
            self.rev = 0;
        }
        self.epoch = epoch.to_string();
    }

    /// Changes from the room: `items` are `{s, c, e, at}`.
    pub fn remote(&mut self, items: &[Value], del: &[Value], rev: Option<u64>) -> Changes {
        let mut out = Changes::default();
        for item in items {
            let Some(record) = item.get("s").filter(|s| s.is_object()) else { continue };
            let Some(id) = record["id"].as_str() else { continue };
            if let Some(seen) = &mut self.full_sync {
                seen.insert(id.to_string());
            }
            self.attribution.insert(
                id.to_string(),
                Attribution {
                    created_by: item["c"].as_str().unwrap_or_default().to_string(),
                    edited_by: item["e"].as_str().unwrap_or_default().to_string(),
                    edited_at: item["at"].as_i64().unwrap_or_default(),
                },
            );
            if self.pending.contains_key(id) {
                continue;
            }
            if self.known.get(id) != Some(record) {
                self.known.insert(id.to_string(), record.clone());
                out.put.push(record.clone());
            }
        }
        for id in del.iter().filter_map(Value::as_str) {
            self.attribution.remove(id);
            if self.pending.contains_key(id) {
                continue;
            }
            if self.known.remove(id).is_some() {
                out.del.push(id.to_string());
            }
        }
        if let Some(rev) = rev {
            self.rev = self.rev.max(rev);
        }
        out
    }

    /// Ends a sync. After a full sync, records the room did not send are gone.
    pub fn synced(&mut self, rev: u64) -> Changes {
        self.rev = rev;
        let mut out = Changes::default();
        if let Some(seen) = self.full_sync.take() {
            let gone: Vec<String> = self.known.keys().filter(|id| !seen.contains(*id) && !self.pending.contains_key(*id)).cloned().collect();
            for id in gone {
                self.known.remove(&id);
                self.attribution.remove(&id);
                out.del.push(id);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, v: i64) -> Value {
        json!({ "id": id, "v": v })
    }

    fn map(records: &[Value]) -> HashMap<String, Value> {
        records.iter().map(|r| (r["id"].as_str().unwrap().to_string(), r.clone())).collect()
    }

    fn item(r: &Value, by: &str) -> Value {
        json!({ "s": r, "c": by, "e": by, "at": 5 })
    }

    #[test]
    fn sends_only_what_changed() {
        let mut r = Replica::default();
        let b = r.local(&map(&[rec("a", 1), rec("b", 1)]));
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].put.len(), 2);
        assert!(r.local(&map(&[rec("a", 1), rec("b", 1)])).is_empty());
        let b = r.local(&map(&[rec("a", 2)]));
        assert_eq!(b[0].put, vec![rec("a", 2)]);
        assert_eq!(b[0].del, vec!["b".to_string()]);
    }

    #[test]
    fn ignores_remote_changes_to_records_it_is_still_sending() {
        let mut r = Replica::default();
        let b = r.local(&map(&[rec("a", 1)]));
        // Someone else's change reached the room first: ours wins there.
        let c = r.remote(&[item(&rec("a", 9), "ben")], &[], Some(1));
        assert!(c.is_empty());
        r.ack(b[0].id, 2, 10, "me");
        assert!(!r.has_pending());
        assert_eq!(r.attribution["a"].edited_by, "me");
        // Afterwards their changes apply.
        let c = r.remote(&[item(&rec("a", 3), "ben")], &[], Some(3));
        assert_eq!(c.put, vec![rec("a", 3)]);
        assert_eq!(r.attribution["a"].edited_by, "ben");
        assert_eq!(r.rev, 3);
    }

    #[test]
    fn a_later_batch_keeps_the_record_pending() {
        let mut r = Replica::default();
        let first = r.local(&map(&[rec("a", 1)]));
        let second = r.local(&map(&[rec("a", 2)]));
        r.ack(first[0].id, 1, 1, "me");
        assert!(r.has_pending());
        assert!(r.remote(&[item(&rec("a", 7), "ben")], &[], None).is_empty());
        r.ack(second[0].id, 3, 1, "me");
        assert!(!r.has_pending());
    }

    #[test]
    fn full_sync_removes_what_the_room_no_longer_has() {
        let mut r = Replica::new("e1".into(), 4, map(&[rec("old", 1), rec("keep", 1)]), HashMap::new());
        r.welcome("e2", true);
        let c = r.remote(&[item(&rec("keep", 1), "ana"), item(&rec("new", 1), "ana")], &[], None);
        assert_eq!(c.put, vec![rec("new", 1)]);
        let gone = r.synced(8);
        assert_eq!(gone.del, vec!["old".to_string()]);
        assert_eq!((r.epoch.as_str(), r.rev), ("e2", 8));
    }

    #[test]
    fn resends_unconfirmed_changes_after_reconnecting() {
        let mut r = Replica::new("e".into(), 1, map(&[rec("gone", 1)]), HashMap::new());
        let now = map(&[rec("a", 1)]);
        r.local(&now);
        // Connection dropped before any ack.
        let again = r.resend(&now);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].put, vec![rec("a", 1)]);
        assert_eq!(again[0].del, vec!["gone".to_string()]);
    }

    #[test]
    fn splits_large_changes() {
        let mut r = Replica::default();
        let big = "x".repeat(200 << 10);
        let records: Vec<Value> = (0..6).map(|i| json!({ "id": format!("r{i}"), "data": big })).collect();
        let batches = r.local(&map(&records));
        assert!(batches.len() >= 3);
        assert_eq!(batches.iter().map(|b| b.put.len()).sum::<usize>(), 6);
        let ids: HashSet<u64> = batches.iter().map(|b| b.id).collect();
        assert_eq!(ids.len(), batches.len());
    }
}
