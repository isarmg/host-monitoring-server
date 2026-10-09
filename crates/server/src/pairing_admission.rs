use std::{
    collections::HashMap,
    hash::Hash,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};

const ENTRY_TTL: Duration = Duration::from_secs(60 * 60);
const SOURCE_CAPACITY: usize = 4_096;
const IDENTIFIER_CAPACITY: usize = 8_192;

/// Bounded admission state for the unauthenticated pairing protocol and the
/// authenticated invite endpoint. Database limits still cap live pending rows;
/// these buckets bound request work before it reaches SQLite.
#[derive(Clone)]
pub(crate) struct PairingAdmission {
    state: Arc<Mutex<AdmissionState>>,
}

impl PairingAdmission {
    pub(crate) fn production() -> Self {
        Self::new(AdmissionPolicies {
            create_source: BucketPolicy::new(24, Duration::from_secs(5)),
            create_device: BucketPolicy::new(6, Duration::from_secs(30)),
            poll_source: BucketPolicy::new(240, Duration::from_millis(250)),
            poll_request: BucketPolicy::new(30, Duration::from_secs(1)),
            activate_source: BucketPolicy::new(16, Duration::from_secs(5)),
            activate_request: BucketPolicy::new(6, Duration::from_secs(30)),
            invite_account: BucketPolicy::new(32, Duration::from_secs(10)),
            source_capacity: SOURCE_CAPACITY,
            identifier_capacity: IDENTIFIER_CAPACITY,
            entry_ttl: ENTRY_TTL,
        })
    }

    fn new(policies: AdmissionPolicies) -> Self {
        Self {
            state: Arc::new(Mutex::new(AdmissionState {
                create_sources: BoundedBuckets::new(
                    policies.create_source,
                    policies.source_capacity,
                    policies.entry_ttl,
                ),
                create_devices: BoundedBuckets::new(
                    policies.create_device,
                    policies.identifier_capacity,
                    policies.entry_ttl,
                ),
                poll_sources: BoundedBuckets::new(
                    policies.poll_source,
                    policies.source_capacity,
                    policies.entry_ttl,
                ),
                poll_requests: BoundedBuckets::new(
                    policies.poll_request,
                    policies.identifier_capacity,
                    policies.entry_ttl,
                ),
                activation_sources: BoundedBuckets::new(
                    policies.activate_source,
                    policies.source_capacity,
                    policies.entry_ttl,
                ),
                activation_requests: BoundedBuckets::new(
                    policies.activate_request,
                    policies.identifier_capacity,
                    policies.entry_ttl,
                ),
                invite_accounts: BoundedBuckets::new(
                    policies.invite_account,
                    policies.identifier_capacity,
                    policies.entry_ttl,
                ),
            })),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(burst: u32, capacity: usize, entry_ttl: Duration) -> Self {
        let policy = BucketPolicy::new(burst, Duration::from_secs(60));
        Self::new(AdmissionPolicies {
            create_source: policy,
            create_device: policy,
            poll_source: policy,
            poll_request: policy,
            activate_source: policy,
            activate_request: policy,
            invite_account: policy,
            source_capacity: capacity,
            identifier_capacity: capacity,
            entry_ttl,
        })
    }

    pub(crate) fn check_create(&self, source: IpAddr, device_id: &str) -> Result<()> {
        let now = Instant::now();
        let mut state = self.lock();
        state
            .create_sources
            .check_at(canonical_ip(source), now)
            .map_err(|delay| limited("pairing source rate exceeded", delay))?;
        state
            .create_devices
            .check_at(identifier_key(device_id), now)
            .map_err(|delay| limited("pairing device rate exceeded", delay))
    }

    pub(crate) fn check_poll(&self, source: IpAddr, request_id: Uuid) -> Result<()> {
        let now = Instant::now();
        let mut state = self.lock();
        state
            .poll_sources
            .check_at(canonical_ip(source), now)
            .map_err(|delay| limited("pairing poll source rate exceeded", delay))?;
        state
            .poll_requests
            .check_at(request_id, now)
            .map_err(|delay| limited("pairing request poll rate exceeded", delay))
    }

    pub(crate) fn check_activation(&self, source: IpAddr, request_id: Uuid) -> Result<()> {
        let now = Instant::now();
        let mut state = self.lock();
        state
            .activation_sources
            .check_at(canonical_ip(source), now)
            .map_err(|delay| limited("pairing activation source rate exceeded", delay))?;
        state
            .activation_requests
            .check_at(request_id, now)
            .map_err(|delay| limited("pairing activation attempts exceeded", delay))
    }

    pub(crate) fn check_invite_account(&self, account_id: &str) -> Result<()> {
        self.lock()
            .invite_accounts
            .check_at(identifier_key(account_id), Instant::now())
            .map_err(|delay| limited("pairing invite account rate exceeded", delay))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AdmissionState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

struct AdmissionPolicies {
    create_source: BucketPolicy,
    create_device: BucketPolicy,
    poll_source: BucketPolicy,
    poll_request: BucketPolicy,
    activate_source: BucketPolicy,
    activate_request: BucketPolicy,
    invite_account: BucketPolicy,
    source_capacity: usize,
    identifier_capacity: usize,
    entry_ttl: Duration,
}

struct AdmissionState {
    create_sources: BoundedBuckets<IpAddr>,
    create_devices: BoundedBuckets<[u8; 32]>,
    poll_sources: BoundedBuckets<IpAddr>,
    poll_requests: BoundedBuckets<Uuid>,
    activation_sources: BoundedBuckets<IpAddr>,
    activation_requests: BoundedBuckets<Uuid>,
    invite_accounts: BoundedBuckets<[u8; 32]>,
}

#[derive(Clone, Copy)]
struct BucketPolicy {
    burst: u32,
    refill_interval: Duration,
}

impl BucketPolicy {
    fn new(burst: u32, refill_interval: Duration) -> Self {
        assert!(burst > 0);
        assert!(!refill_interval.is_zero());
        Self {
            burst,
            refill_interval,
        }
    }
}

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    last_seen: Instant,
}

impl Bucket {
    fn new(policy: BucketPolicy, now: Instant) -> Self {
        Self {
            tokens: f64::from(policy.burst),
            last_refill: now,
            last_seen: now,
        }
    }

    fn tokens_at(self, policy: BucketPolicy, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(self.last_refill);
        (self.tokens + elapsed.as_secs_f64() / policy.refill_interval.as_secs_f64())
            .min(f64::from(policy.burst))
    }

    fn check(&mut self, policy: BucketPolicy, now: Instant) -> std::result::Result<(), Duration> {
        self.tokens = self.tokens_at(policy, now);
        self.last_refill = now;
        self.last_seen = now;
        if self.tokens < 1.0 {
            let missing = 1.0 - self.tokens;
            return Err(Duration::from_secs_f64(
                missing * policy.refill_interval.as_secs_f64(),
            ));
        }
        self.tokens -= 1.0;
        Ok(())
    }
}

struct BoundedBuckets<K> {
    entries: HashMap<K, Bucket>,
    policy: BucketPolicy,
    capacity: usize,
    entry_ttl: Duration,
}

impl<K> BoundedBuckets<K>
where
    K: Clone + Eq + Hash,
{
    fn new(policy: BucketPolicy, capacity: usize, entry_ttl: Duration) -> Self {
        assert!(capacity > 0);
        assert!(!entry_ttl.is_zero());
        Self {
            entries: HashMap::new(),
            policy,
            capacity,
            entry_ttl,
        }
    }

    fn check_at(&mut self, key: K, now: Instant) -> std::result::Result<(), Duration> {
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.last_seen) < self.entry_ttl);
        if !self.entries.contains_key(&key) {
            self.make_room(now)?;
            self.entries
                .insert(key.clone(), Bucket::new(self.policy, now));
        }
        self.entries
            .get_mut(&key)
            .expect("the bucket exists")
            .check(self.policy, now)
    }

    fn make_room(&mut self, now: Instant) -> std::result::Result<(), Duration> {
        if self.entries.len() < self.capacity {
            return Ok(());
        }
        let evictable = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.tokens_at(self.policy, now) >= f64::from(self.policy.burst))
            .min_by_key(|(_, entry)| entry.last_seen)
            .map(|(key, _)| key.clone());
        if let Some(key) = evictable {
            self.entries.remove(&key);
            return Ok(());
        }
        let retry = self
            .entries
            .values()
            .map(|entry| {
                let missing = f64::from(self.policy.burst) - entry.tokens_at(self.policy, now);
                Duration::from_secs_f64(
                    missing.max(0.0) * self.policy.refill_interval.as_secs_f64(),
                )
            })
            .min()
            .unwrap_or(self.entry_ttl);
        Err(retry.max(Duration::from_nanos(1)))
    }
}

fn identifier_key(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn canonical_ip(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
        IpAddr::V4(_) => address,
    }
}

fn limited(message: &'static str, delay: Duration) -> Error {
    Error::RateLimited {
        message,
        retry_after: delay
            .as_secs()
            .saturating_add(u64::from(delay.subsec_nanos() != 0))
            .max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_limits_source_and_device_independently() {
        let admission = PairingAdmission::for_test(1, 8, Duration::from_secs(300));
        let first: IpAddr = "192.0.2.1".parse().unwrap();
        let second: IpAddr = "192.0.2.2".parse().unwrap();
        let third: IpAddr = "192.0.2.3".parse().unwrap();
        admission.check_create(first, "device-a").unwrap();
        assert!(matches!(
            admission.check_create(first, "device-b"),
            Err(Error::RateLimited {
                message: "pairing source rate exceeded",
                ..
            })
        ));
        assert!(matches!(
            admission.check_create(second, "device-a"),
            Err(Error::RateLimited {
                message: "pairing device rate exceeded",
                ..
            })
        ));
        admission.check_create(third, "device-b").unwrap();
    }

    #[test]
    fn request_and_account_maps_are_bounded_and_return_retry_after() {
        let admission = PairingAdmission::for_test(1, 1, Duration::from_secs(300));
        let source: IpAddr = "192.0.2.10".parse().unwrap();
        admission.check_poll(source, Uuid::new_v4()).unwrap();
        let error = admission
            .check_poll("192.0.2.11".parse().unwrap(), Uuid::new_v4())
            .unwrap_err();
        assert!(matches!(
            error,
            Error::RateLimited {
                retry_after: 60,
                ..
            }
        ));

        admission.check_invite_account("account-a").unwrap();
        assert!(matches!(
            admission.check_invite_account("account-b"),
            Err(Error::RateLimited { .. })
        ));
    }

    #[test]
    fn ipv4_mapped_source_cannot_reset_a_bucket() {
        let admission = PairingAdmission::for_test(1, 8, Duration::from_secs(300));
        let v4: IpAddr = "192.0.2.20".parse().unwrap();
        let mapped = IpAddr::V6(std::net::Ipv4Addr::new(192, 0, 2, 20).to_ipv6_mapped());
        admission.check_activation(v4, Uuid::new_v4()).unwrap();
        assert!(matches!(
            admission.check_activation(mapped, Uuid::new_v4()),
            Err(Error::RateLimited { .. })
        ));
    }
}
