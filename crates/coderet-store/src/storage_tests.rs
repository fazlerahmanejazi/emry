#[cfg(test)]
mod tests {
    use crate::Store;
    use anyhow::Result;
    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestStruct {
        id: u32,
        name: String,
    }

    #[test]
    fn test_encoded_operations() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let store = Store::open(temp_dir.path())?;
        let tree = store.open_tree("test_tree")?;

        let val = TestStruct {
            id: 1,
            name: "test".to_string(),
        };

        tree.insert_encoded("key1", &val)?;

        let retrieved: Option<TestStruct> = tree.get_decoded("key1")?;
        assert_eq!(retrieved, Some(val));

        let missing: Option<TestStruct> = tree.get_decoded("key2")?;
        assert_eq!(missing, None);

        Ok(())
    }
}
