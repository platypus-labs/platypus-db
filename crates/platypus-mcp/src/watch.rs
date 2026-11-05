//! Schema change notifications: `resources/subscribe` on the schema resources, and
//! `notifications/resources/updated` and `list_changed` when the app saw a change.
//!
//! The server may be another process than the app, so the two meet in a file: the app
//! bumps a counter for the connection in [`STAMP_FILE`] (in the profile's data folder) when
//! it refreshes the navigator or stores a schema snapshot that differs from the last, and a
//! server with subscribers reads the file every [`WATCH_EVERY`]. With no subscriber nothing
//! runs; the task stops when the last one unsubscribes and when the server goes away. A
//! change made outside the app is seen at the app's next refresh.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::ResourceUpdatedNotificationParam;
use rmcp::{Peer, RoleServer};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::Agent;
use crate::agent::locked;
use crate::resource::{self, Target};

/// The file in the data folder holding, per connection id, how many times the app saw its
/// schema change.
pub const STAMP_FILE: &str = "schema-changes.json";

/// Overrides [`WATCH_EVERY`], in milliseconds (tests).
pub const WATCH_MS_ENV: &str = "PLATYPUS_MCP_WATCH_MS";

/// How often a server with subscribers looks at the stamp file.
pub const WATCH_EVERY: Duration = Duration::from_secs(5);

type Stamps = HashMap<Uuid, u64>;

fn read_stamps(dir: &Path) -> Stamps {
    std::fs::read_to_string(dir.join(STAMP_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<HashMap<String, u64>>(&text).ok())
        .map(|map| {
            map.into_iter()
                .filter_map(|(id, n)| Some((Uuid::parse_str(&id).ok()?, n)))
                .collect()
        })
        .unwrap_or_default()
}

/// Records that the app saw the schema of `connection` change. Called by the app.
pub fn note_change(dir: &Path, connection: Uuid) -> std::io::Result<()> {
    let mut stamps = read_stamps(dir);
    let n = stamps.entry(connection).or_default();
    *n = n.wrapping_add(1);
    let map: HashMap<String, u64> = stamps
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let text = serde_json::to_string(&map).unwrap_or_default();
    // Written aside and renamed, so a server never reads half a file.
    let temp = dir.join(format!("{STAMP_FILE}.tmp"));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, dir.join(STAMP_FILE))
}

fn every() -> Duration {
    std::env::var(WATCH_MS_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(WATCH_EVERY, Duration::from_millis)
}

/// The resources a client subscribed to, and the task that watches for changes to them.
#[derive(Default)]
pub struct Subscriptions {
    uris: Arc<Mutex<BTreeSet<String>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for Subscriptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscriptions")
            .field("uris", &*locked(&self.uris))
            .finish_non_exhaustive()
    }
}

impl Drop for Subscriptions {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Subscriptions {
    /// Subscribes to `uri` and starts the watching if it is the first.
    pub fn add(&self, uri: &str, agent: &Arc<Agent>, peer: &Peer<RoleServer>) {
        locked(&self.uris).insert(uri.to_owned());
        let mut task = locked(&self.task);
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        // Read before the task starts: a change right after the subscription is seen.
        let seen = read_stamps(agent.profile().data_dir());
        *task = Some(tokio::spawn(watch(
            Arc::clone(agent),
            Arc::clone(&self.uris),
            peer.clone(),
            seen,
            every(),
        )));
    }

    /// Unsubscribes from `uri`; the watching stops with the last subscription.
    pub fn remove(&self, uri: &str) {
        let empty = {
            let mut uris = locked(&self.uris);
            uris.remove(uri);
            uris.is_empty()
        };
        if empty {
            self.stop();
        }
    }

    fn stop(&self) {
        if let Some(task) = locked(&self.task).take() {
            task.abort();
        }
    }
}

/// Whether the resource `uri` is of a connection in `changed` that agents may still use.
fn concerns(agent: &Agent, uri: &str, changed: &[Uuid]) -> bool {
    let Some(Target::Connection(c) | Target::Schema(c, _) | Target::Table(c, _, _)) =
        resource::parse(uri)
    else {
        return false;
    };
    // `find` fails for a connection whose access is off: it never notifies.
    agent
        .find(&c)
        .is_ok_and(|(_, saved)| changed.contains(&saved.id))
}

async fn watch(
    agent: Arc<Agent>,
    uris: Arc<Mutex<BTreeSet<String>>>,
    peer: Peer<RoleServer>,
    mut seen: Stamps,
    period: Duration,
) {
    let dir = agent.profile().data_dir().to_path_buf();
    let mut tick = tokio::time::interval(period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await;
    loop {
        tick.tick().await;
        let now = read_stamps(&dir);
        let changed: Vec<Uuid> = now
            .iter()
            .filter(|(id, n)| seen.get(id) != Some(n))
            .map(|(id, _)| *id)
            .collect();
        seen = now;
        if changed.is_empty() {
            continue;
        }
        let subscribed: Vec<String> = locked(&uris).iter().cloned().collect();
        let mut notified = false;
        for uri in subscribed {
            if concerns(&agent, &uri, &changed) {
                if peer
                    .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri))
                    .await
                    .is_err()
                {
                    return;
                }
                notified = true;
            }
        }
        if notified && peer.notify_resource_list_changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stamp_counts_per_connection() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        assert!(read_stamps(dir.path()).is_empty());
        note_change(dir.path(), a).unwrap();
        note_change(dir.path(), a).unwrap();
        note_change(dir.path(), b).unwrap();
        let stamps = read_stamps(dir.path());
        assert_eq!((stamps[&a], stamps[&b]), (2, 1));
    }

    #[test]
    fn a_broken_stamp_file_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STAMP_FILE), "{ not json").unwrap();
        assert!(read_stamps(dir.path()).is_empty());
        // The next change replaces it.
        note_change(dir.path(), Uuid::new_v4()).unwrap();
        assert_eq!(read_stamps(dir.path()).len(), 1);
    }
}
