extern crate lru_cache;

use std::borrow::Borrow;
use std::hash::Hash;
use lru_cache::LruCache;

pub struct ArcCache<K, V> where K: Eq+Hash {
    recent_set: LruCache<K, V>,
    recent_evicted: LruCache<K, ()>,
    frequent_set: LruCache<K, V>,
    frequent_evicted: LruCache<K, ()>,
    capacity: usize,
    p: usize
}

impl<K, V> ArcCache<K, V> where K: Eq+Hash {
    pub fn new(capacity: usize) -> ArcCache<K, V> {
        ArcCache {
            recent_set: LruCache::new(capacity),
            recent_evicted: LruCache::new(capacity),
            frequent_set: LruCache::new(capacity),
            frequent_evicted: LruCache::new(capacity),
            capacity: capacity,
            p: 0
        }
    }

    pub fn contains_key<Q: ?Sized>(&mut self, key: &Q) -> bool where K: Borrow<Q>, Q: Hash + Eq {
        self.recent_set.contains_key(key) || self.frequent_set.contains_key(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> bool {
        if self.recent_set.contains_key(&key) {
            self.recent_set.remove(&key);
            self.frequent_set.insert(key, value);
            return true
        }
        if self.frequent_set.contains_key(&key) {
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
        if self.frequent_evicted.contains_key(&key) {
            let recent_evicted_len = self.recent_evicted.len();
            let frequent_evicted_len = self.frequent_evicted.len();
            let delta = if recent_evicted_len > frequent_evicted_len {
                recent_evicted_len / frequent_evicted_len
            } else {
                1
            };
            if delta > self.p {
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
        if self.recent_set.len() + self.frequent_set.len() >= self.capacity {
            self.replace(false);
        }
        if self.recent_evicted.len() > self.capacity - self.p {
            self.recent_evicted.remove_lru();
        }
        if self.frequent_evicted.len() > self.p {
            self.frequent_evicted.remove_lru();
        }
        self.recent_set.insert(key, value);
        false
    }

    pub fn peek_mut<Q: ?Sized>(&mut self, key: &K) -> Option<&mut V> {
        if let Some(entry) = self.recent_set.peek_mut(key) {
            Some(entry)
        } else {
            self.frequent_set.peek_mut(key)
        }
    }

    pub fn get_mut<Q: ?Sized>(&mut self, key: K) -> Option<&mut V> where K: Clone {
        if let Some(value) = self.recent_set.remove(&key) {
            self.frequent_set.insert(key.clone(), value);
        }
        self.frequent_set.get_mut(&key)
    }

    fn replace(&mut self, frequent_evicted_contains_key: bool) {
        let recent_set_len = self.recent_set.len();
        if recent_set_len > 0 && (recent_set_len > self.p || (recent_set_len == self.p && frequent_evicted_contains_key)) {
            if let Some((old_key, _)) = self.recent_set.remove_lru() {
                self.recent_evicted.insert(old_key, ());
            }
        } else {
            if let Some((old_key, _)) = self.frequent_set.remove_lru() {
                self.frequent_evicted.insert(old_key, ());
            }
        }
    }
}

#[test]
fn test_arc() {
    let mut arc: ArcCache<&str,&str> = ArcCache::new(2);
    arc.insert("testkey", "testvalue");
    assert!(arc.contains_key(&"testkey"));
    arc.insert("testkey2", "testvalue2");
    assert!(arc.contains_key(&"testkey2"));
    arc.insert("testkey3", "testvalue3");
    assert!(arc.contains_key(&"testkey3"));
    assert!(arc.contains_key(&"testkey2"));
    assert!(!arc.contains_key(&"testkey"));
}
