
#[cfg(test)]
mod tests {
    use crate::relations::{extract_calls_imports, RelationRef};
    use crate::models::Language;

    #[test]
    fn test_extract_rust_method_calls() {
        let code = r#"
            fn my_func() {
                let x = foo();
                let y = obj.bar();
                let z = obj.nested.baz();
                let w = Self::static_method();
            }
        "#;

        let (calls, _) = extract_calls_imports(&Language::Rust, code);
        
        let call_names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
        println!("Found calls: {:?}", call_names);

        assert!(call_names.contains(&"foo"));
        assert!(call_names.contains(&"bar"));
        assert!(call_names.contains(&"baz"));
        assert!(call_names.contains(&"static_method"));
    }
}
