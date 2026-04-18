use crate::parse::EvalResult;

pub fn to_json(result: &EvalResult) -> String {
    serde_json::to_string(result)
        .unwrap_or_else(|e| format!(r#"{{"error":"serialization failed: {e}"}}"#))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_json_output_structure() {
        let r = EvalResult {
            expr: "map (+1) [1,2,3]".into(),
            type_str: Some("[Integer]".into()),
            value: "[2,3,4]".into(),
            diagnostics: vec![],
        };
        let j: serde_json::Value = serde_json::from_str(&to_json(&r)).unwrap();
        assert_eq!(j["expr"], "map (+1) [1,2,3]");
        assert_eq!(j["type"], "[Integer]");
        assert_eq!(j["value"], "[2,3,4]");
        assert_eq!(j["diagnostics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_json_with_diagnostics() {
        let r = EvalResult {
            expr: "foo".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![parse::simple_diagnostic("error", "not in scope".into())],
        };
        let j: serde_json::Value = serde_json::from_str(&to_json(&r)).unwrap();
        assert!(j["type"].is_null());
        assert_eq!(j["diagnostics"][0]["severity"], "error");
    }

    #[test]
    fn test_json_null_type() {
        let r = EvalResult {
            expr: "putStrLn \"hi\"".into(),
            type_str: None,
            value: "hi".into(),
            diagnostics: vec![],
        };
        let j: serde_json::Value = serde_json::from_str(&to_json(&r)).unwrap();
        assert!(j["type"].is_null());
        assert_eq!(j["value"], "hi");
    }
}
