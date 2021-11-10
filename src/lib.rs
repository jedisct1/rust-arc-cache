use std::borrow::Borrow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash};
use std::iter::Chain;
use xlru_cache::LruCache;

pub struct ArcCache<K, V, S = RandomState>
where
    K: Eq + Hash,
    S: BuildHasher,
{
    recent_set: LruCache<K, V, S>,
    recent_evicted: LruCache<K, (), S>,
    frequent_set: LruCache<K, V, S>,
    frequent_evicted: LruCache<K, (), S>,
    capacity: usize,
    p: usize,
    inserted: u64,
    evicted: u64,
    removed: u64,
}

/// An iterator over all items in the cache. Iterates over frequently-used items
/// followed by recently-used items. Within each of these groups, the iterator
/// yields less recently used items first.
pub struct ArcCacheIterator<'a, K, V>(
    Chain<xlru_cache::Iter<'a, K, V>, xlru_cache::Iter<'a, K, V>>,
)
where
    K: Eq + Hash;

impl<K, V> ArcCache<K, V>
where
    K: Eq + Hash,
{
    pub fn new(capacity: usize) -> Result<Self, &'static str> {
        if capacity == 0 {
            return Err("Cache length cannot be zero");
        }
        let cache = ArcCache {
            recent_set: LruCache::new(capacity),
            recent_evicted: LruCache::new(capacity),
            frequent_set: LruCache::new(capacity),
            frequent_evicted: LruCache::new(capacity),
            capacity,
            p: 0,
            inserted: 0,
            evicted: 0,
            removed: 0,
        };
        Ok(cache)
    }
}

impl<K, V, S> ArcCache<K, V, S>
where
    K: Eq + Hash,
    S: BuildHasher,
{
    /// Creates an empty cache that can hold at most `capacity` items with the given hash builder.
    pub fn with_hasher(capacity: usize, hash_builder: S) -> Result<Self, &'static str>
    where
        S: Clone,
    {
        if capacity == 0 {
            return Err("Cache length cannot be zero");
        }
        let cache = ArcCache {
            recent_set: LruCache::with_hasher(capacity, hash_builder.clone()),
            recent_evicted: LruCache::with_hasher(capacity, hash_builder.clone()),
            frequent_set: LruCache::with_hasher(capacity, hash_builder.clone()),
            frequent_evicted: LruCache::with_hasher(capacity, hash_builder),
            capacity,
            p: 0,
            inserted: 0,
            evicted: 0,
            removed: 0,
        };
        Ok(cache)
    }

    pub fn contains_key<Q: ?Sized>(&mut self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.frequent_set.contains_key(key) || self.recent_set.contains_key(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> bool {
        if self.frequent_set.contains_key(&key) {
            self.frequent_set.insert(key, value);
            return true;
        }
        if self.recent_set.contains_key(&key) {
            self.recent_set.remove(&key);
            self.frequent_set.insert(key, value);
            return true;
        }
        if self.frequent_evicted.contains_key(&key) {
            let recent_evicted_len = self.recent_evicted.len();
            let frequent_evicted_len = self.frequent_evicted.len();
            let delta = if recent_evicted_len > frequent_evicted_len {
                recent_evicted_len / frequent_evicted_len
            } else {
                1
            };
            if delta < self.p {
                self.p -= delta;
            } else {
                self.p = 0
            }
            if self.recent_set.len() + self.frequent_set.len() >= self.capacity {
                self.replace(true);
            }
            self.frequent_evicted.remove(&key);
            self.frequent_set.insert(key, value);
            return true;
        }
        if self.recent_evicted.contains_key(&key) {
            let recent_evicted_len = self.recent_evicted.len();
            let frequent_evicted_len = self.frequent_evicted.len();
            let delta = if frequent_evicted_len > recent_evicted_len {
                frequent_evicted_len / recent_evicted_len
            } else {
                1
            };
            if delta <= self.capacity - self.p {
                self.p += delta;
            } else {
                self.p = self.capacity;
            }
            if self.recent_set.len() + self.frequent_set.len() >= self.capacity {
                self.replace(false);
            }
            self.recent_evicted.remove(&key);
            self.frequent_set.insert(key, value);
            return true;
        }
        if self.recent_set.len() + self.frequent_set.len() >= self.capacity {
            self.replace(false);
        }
        if self.recent_evicted.len() > self.capacity - self.p {
            self.recent_evicted.remove_lru();
            self.evicted += 1;
        }
        if self.frequent_evicted.len() > self.p {
            self.frequent_evicted.remove_lru();
            self.evicted += 1;
        }
        self.recent_set.insert(key, value);
        self.inserted += 1;
        false
    }

    pub fn peek_mut(&mut self, key: &K) -> Option<&mut V>
    where
        K: Clone + Hash + Eq,
    {
        if let Some(entry) = self.frequent_set.peek_mut(key) {
            Some(entry)
        } else {
            self.recent_set.peek_mut(key)
        }
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V>
    where
        K: Clone + Hash + Eq,
    {
        if let Some(value) = self.recent_set.remove(key) {
            self.frequent_set.insert((*key).clone(), value);
        }
        self.frequent_set.get_mut(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let removed_frequent = self.frequent_set.remove(key);
        let removed_recent = self.recent_set.remove(key);

        self.frequent_evicted.remove(key);
        self.recent_evicted.remove(key);

        match removed_frequent.or(removed_recent) {
            Some(value) => {
                self.removed += 1;
                Some(value)
            }
            None => None,
        }
    }

    fn replace(&mut self, frequent_evicted_contains_key: bool) {
        let recent_set_len = self.recent_set.len();
        if recent_set_len > 0
            && (recent_set_len > self.p
                || (recent_set_len == self.p && frequent_evicted_contains_key))
        {
            if let Some((old_key, _)) = self.recent_set.remove_lru() {
                self.recent_evicted.insert(old_key, ());
            }
        } else if let Some((old_key, _)) = self.frequent_set.remove_lru() {
            self.frequent_evicted.insert(old_key, ());
        }
    }

    pub fn clear(&mut self) {
        self.recent_set.clear();
        self.recent_evicted.clear();
        self.frequent_set.clear();
        self.frequent_evicted.clear();
    }

    pub fn len(&self) -> usize {
        self.recent_set.len() + self.frequent_set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn frequent_len(&self) -> usize {
        self.frequent_set.len()
    }

    pub fn recent_len(&self) -> usize {
        self.recent_set.len()
    }

    pub fn inserted(&self) -> u64 {
        self.inserted
    }

    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    pub fn removed(&self) -> u64 {
        self.removed
    }
}

impl<'a, K, V, S> IntoIterator for &'a ArcCache<K, V, S>
where
    K: Eq + Hash,
    S: BuildHasher,
{
    type Item = (&'a K, &'a V);
    type IntoIter = ArcCacheIterator<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        ArcCacheIterator(self.frequent_set.iter().chain(self.recent_set.iter()))
    }
}

impl<'a, K, V> Iterator for ArcCacheIterator<'a, K, V>
where
    K: Eq + Hash,
{
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

#[test]
fn test_arc() {
    let mut arc: ArcCache<&str, &str> = ArcCache::new(2).unwrap();
    arc.insert("testkey", "testvalue");
    assert!(arc.contains_key(&"testkey"));
    arc.insert("testkey2", "testvalue2");
    assert!(arc.contains_key(&"testkey2"));
    arc.insert("testkey3", "testvalue3");
    assert!(arc.contains_key(&"testkey3"));
    assert!(arc.contains_key(&"testkey2"));
    assert!(!arc.contains_key(&"testkey"));
    arc.insert("testkey", "testvalue");
    assert!(arc.get_mut(&"testkey").is_some());
    assert!(arc.get_mut(&"testkey-nx").is_none());

    let mut it = arc.into_iter();
    assert_eq!(it.next(), Some((&"testkey", &"testvalue")));
    assert_eq!(it.next(), Some((&"testkey2", &"testvalue2")));
    assert_eq!(it.next(), None);
}

#[test]
fn test_custom_hasher() {
    use nohash_hasher::NoHashHasher;
    use std::hash::BuildHasherDefault;

    let mut arc: ArcCache<u32, &str, BuildHasherDefault<NoHashHasher<u8>>> =
        ArcCache::with_hasher(2, BuildHasherDefault::default()).unwrap();
    arc.insert(1, "testvalue");
    assert!(arc.contains_key(&1));
    arc.insert(2, "testvalue2");
    assert!(arc.contains_key(&2));
    arc.insert(3, "testvalue3");
    assert!(arc.contains_key(&3));
    assert!(arc.contains_key(&2));
    assert!(!arc.contains_key(&1));
    arc.insert(1, "testvalue");
    assert!(arc.get_mut(&1).is_some());
    assert!(arc.get_mut(&666).is_none());

    let mut it = arc.into_iter();
    assert_eq!(it.next(), Some((&1, &"testvalue")));
    assert_eq!(it.next(), Some((&2, &"testvalue2")));
    assert_eq!(it.next(), None);
}
