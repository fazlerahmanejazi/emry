use anyhow::{Context, Result};
use std::path::Path;

/// Opaque wrapper around the underlying storage engine (sled).
#[derive(Clone)]
pub struct Store {
    db: sled::Db,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = sled::open(path).context("failed to open store")?;
        Ok(Self { db })
    }

    pub fn open_tree(&self, name: &str) -> Result<Tree> {
        let tree = self.db.open_tree(name)?;
        Ok(Tree { inner: tree })
    }
}

/// Opaque wrapper around a storage keyspace/tree.
#[derive(Clone)]
pub struct Tree {
    inner: sled::Tree,
}

impl Tree {
    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
        Ok(self.inner.get(key)?.map(|iv| iv.to_vec()))
    }

    pub fn insert<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) -> Result<()> {
        self.inner.insert(key, value.as_ref())?;
        Ok(())
    }

    pub fn remove<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>> {
        Ok(self.inner.remove(key)?.map(|iv| iv.to_vec()))
    }

    pub fn contains_key<K: AsRef<[u8]>>(&self, key: K) -> Result<bool> {
        Ok(self.inner.contains_key(key)?)
    }

    pub fn iter(&self) -> Iter {
        Iter {
            inner: self.inner.iter(),
        }
    }

    pub fn scan_prefix<P: AsRef<[u8]>>(&self, prefix: P) -> Iter {
        Iter {
            inner: self.inner.scan_prefix(prefix),
        }
    }

    pub fn name(&self) -> String {
        String::from_utf8_lossy(&self.inner.name()).to_string()
    }
    
    pub fn last(&self) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        Ok(self.inner.last()?.map(|(k, v)| (k.to_vec(), v.to_vec())))
    }
}

pub struct Iter {
    inner: sled::Iter,
}

impl Iterator for Iter {
    type Item = Result<(Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next() {
            Some(Ok((k, v))) => Some(Ok((k.to_vec(), v.to_vec()))),
            Some(Err(e)) => Some(Err(anyhow::Error::new(e))),
            None => None,
        }
    }
}

impl DoubleEndedIterator for Iter {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self.inner.next_back() {
            Some(Ok((k, v))) => Some(Ok((k.to_vec(), v.to_vec()))),
            Some(Err(e)) => Some(Err(anyhow::Error::new(e))),
            None => None,
        }
    }
}
