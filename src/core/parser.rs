// src/core/parser.rs
use crate::core::graph::WorkflowGraph;
use crate::core::jwl_lexer::Lexer;
use crate::core::jwl_parser::JwlParser;
use anyhow::{anyhow, Result};
use std::collections::HashMap;

pub struct GraphParser;

impl GraphParser {
    /// Static helper: parse task parameter string
    /// Handles strings like `key1=value1, key2=[nested]`
    pub fn parse_arguments_str(args_str: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        let mut buffer = String::new();
        let mut key = String::new();
        let mut depth = 0;
        let mut in_quote = false;
        let mut parsing_key = true;

        let chars: Vec<char> = args_str.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            let c = chars[i];

            match c {
                // Handle key-value separator '='
                '=' if depth == 0 && !in_quote && parsing_key => {
                    key = buffer.trim().to_string();
                    buffer.clear();
                    parsing_key = false;
                }
                // Handle parameter separator ','
                ',' if depth == 0 && !in_quote => {
                    if !key.is_empty() {
                        params.insert(key.clone(), buffer.trim().to_string());
                    }
                    buffer.clear();
                    key.clear();
                    parsing_key = true;
                }
                // Handle quotes
                '"' if depth == 0 => {
                    in_quote = !in_quote;
                    buffer.push(c);
                }
                // Handle escaped quotes (inside quoted strings)
                '\\' if in_quote && i + 1 < len && chars[i + 1] == '"' => {
                    buffer.push(c);
                    buffer.push(chars[i + 1]);
                    i += 1; // skip next character
                }
                // Handle nested structures: parentheses, brackets, braces
                '(' | '{' | '[' if !in_quote => {
                    depth += 1;
                    buffer.push(c);
                }
                ')' | '}' | ']' if !in_quote => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    buffer.push(c);
                }
                // Regular character
                _ => {
                    // All characters are collected; trimming handles leading/trailing whitespace
                    buffer.push(c);
                }
            }
            i += 1;
        }

        // Handle the last parameter
        if !key.is_empty() {
            params.insert(key, buffer.trim().to_string());
        }
        params
    }

    pub fn parse(content: &str) -> Result<WorkflowGraph> {
        Self::parse_rdp(content)
    }

    /// Parse .jgflow manifest file — standalone ManifestParser, returns Manifest
    pub fn parse_manifest(content: &str) -> Result<crate::core::graph::Manifest> {
        crate::core::manifest_parser::ManifestParser::parse(content)
    }

    /// Parse library file — allows containing only function definitions, no entry node or regular nodes required
    pub fn parse_lib(content: &str) -> Result<WorkflowGraph> {
        Self::parse_lib_rdp(content)
    }

    fn parse_rdp(content: &str) -> Result<WorkflowGraph> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("JWL Compilation Syntax Error:\n{}", e))?;
        let mut parser = JwlParser::new(&tokens, content);
        let mut wf = parser.parse_workflow()?;

        if wf.entry_node.is_empty() {
            // Find topological entry: a node with in-degree 0
            let entry_idx = wf
                .graph
                .node_indices()
                .find(|&idx| {
                    wf.graph
                        .neighbors_directed(idx, petgraph::Direction::Incoming)
                        .next()
                        .is_none()
                })
                .or_else(|| wf.graph.node_indices().next());

            if let Some(idx) = entry_idx {
                wf.entry_node = wf.graph[idx].id.clone();
            } else {
                return Err(anyhow!(
                    "Architecture Error: Workflow must contain at least one valid node."
                ));
            }
        }
        Ok(wf)
    }

    fn parse_lib_rdp(content: &str) -> Result<WorkflowGraph> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("JWL Compilation Syntax Error:\n{}", e))?;
        let mut parser = JwlParser::new(&tokens, content);
        let wf = parser.parse_workflow()?;

        if wf.entry_node.is_empty()
            && wf.graph.node_indices().next().is_none()
            && wf.functions.is_empty()
        {
            return Err(anyhow!(
                "Library Error: Library file must define at least one function node."
            ));
        }
        Ok(wf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::NodeType;

    #[test]
    fn test_python_imports_parsing() {
        let content = r#"
python: ["pandas", "sklearn.ensemble", "./utils.py"]

[load]: pandas.read_csv(path="data.csv")
[train]: sklearn.ensemble.RandomForestClassifier()

[load] -> [train]
"#;
        let graph = GraphParser::parse(content).unwrap();

        assert_eq!(graph.python_imports.len(), 3);
        assert!(graph.python_imports.contains(&"pandas".to_string()));
        assert!(graph
            .python_imports
            .contains(&"sklearn.ensemble".to_string()));
        assert!(graph.python_imports.contains(&"./utils.py".to_string()));
    }

    #[test]
    fn test_scoped_identifier_call() {
        let content = r#"
python: ["pandas"]

[load]: pandas.read_csv(path="data.csv", encoding="utf-8")
"#;
        let graph = GraphParser::parse(content).unwrap();

        // Verify the node was parsed correctly
        let node = graph.graph.node_weights().next().unwrap();
        assert_eq!(node.id, "load");

        if let NodeType::Task(action) = &node.node_type {
            assert_eq!(action.name, "pandas.read_csv");
            assert_eq!(action.params.get("path"), Some(&"\"data.csv\"".to_string()));
            assert_eq!(
                action.params.get("encoding"),
                Some(&"\"utf-8\"".to_string())
            );
        } else {
            panic!("Expected Task node type");
        }
    }

    #[test]
    fn test_switch_syntax_parsing() {
        let content = r#"
[start]: notify(message="start")
[case_a]: notify(message="A")
[case_b]: notify(message="B")
[fallback]: notify(message="default")

[start] -> switch $type {
    "a": [case_a]
    "b": [case_b]
    default: [fallback]
}
"#;
        let graph = GraphParser::parse(content).unwrap();

        // Verify switch route was created
        assert!(graph.switch_routes.contains_key("start"));
        let switch_route = graph.switch_routes.get("start").unwrap();
        assert_eq!(switch_route.subject.trim(), "$type");
        assert_eq!(switch_route.cases.len(), 3);

        // Verify cases
        assert_eq!(switch_route.cases[0].value, Some("a".to_string()));
        assert_eq!(switch_route.cases[0].target, "case_a");
        assert_eq!(switch_route.cases[1].value, Some("b".to_string()));
        assert_eq!(switch_route.cases[1].target, "case_b");
        assert_eq!(switch_route.cases[2].value, None); // default
        assert_eq!(switch_route.cases[2].target, "fallback");
    }

    #[test]
    fn test_missing_comma_detected() {
        let content = r#"
[start]: notify(message="hello" status="ok")
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_err(),
            "Missing comma between parameters should cause parse error"
        );
    }

    #[test]
    fn test_valid_comma_separated_params() {
        let content = r#"
[start]: notify(message="hello", status="ok")
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_ok(),
            "Comma-separated params should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_comparison_in_expression() {
        // == in edge conditions should still work
        let content = r#"
[start]: notify(message="test")
[a]: notify(message="a")
[b]: notify(message="b")
[start] if $output.category == "technical" -> [a]
[start] -> [b]
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_ok(),
            "Comparison operators should be valid: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_duplicate_param_detected() {
        let content = r#"
[start]: notify(message="first", message="second")
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_err(),
            "Duplicate parameter keys should cause parse error"
        );
    }

    #[test]
    fn test_multiline_params_with_commas() {
        let content = r#"
[start]: chat(
  agent="helper",
  message=$input.query
)
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_ok(),
            "Multiline params should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_single_step_function() {
        let content = r#"
[greet(name)]: bash(command="echo Hello, " + $name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert!(graph.functions.contains_key("greet"));
        let func = graph.functions.get("greet").unwrap();
        assert_eq!(func.params, vec!["name"]);
        assert_eq!(func.body.node_map.len(), 1);
    }

    #[test]
    fn test_multi_step_function() {
        let content = r#"
[build(dir)]: {
  bash(command="cd " + $dir + " && make")
  bash(command="cd " + $dir + " && make test")
}
[step1]: build(dir="/app")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert!(graph.functions.contains_key("build"));
        let func = graph.functions.get("build").unwrap();
        assert_eq!(func.params, vec!["dir"]);
        assert_eq!(func.body.node_map.len(), 2);
        // Verify sequential edge exists
        assert_eq!(func.body.graph.edge_count(), 1);
    }

    #[test]
    fn test_multi_step_function_with_semicolons() {
        let content = r#"
[build(a, b)]: { bash(command=$a); bash(command=$b) }
[step1]: build(a="foo", b="bar")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let func = graph.functions.get("build").unwrap();
        assert_eq!(func.params, vec!["a", "b"]);
        assert_eq!(func.body.node_map.len(), 2);
    }

    #[test]
    fn test_function_not_in_main_graph() {
        let content = r#"
[greet(name)]: bash(command="echo " + $name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        // Function node should NOT be in main graph
        assert!(!graph.node_map.contains_key("greet"));
        // But the caller should be
        assert!(graph.node_map.contains_key("step1"));
    }

    #[test]
    fn test_no_params_backward_compat() {
        let content = r#"
[start]: bash(command="echo hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert!(graph.node_map.contains_key("start"));
        assert!(graph.functions.is_empty());
    }

    #[test]
    fn test_string_concat_expression() {
        let content = r#"
[start]: chat(agent="helper", message="[Expert] " + $input.query)
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_ok(),
            "String concat should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_foreach_without_dollar() {
        let content = r#"
[loop]: foreach(item in input.items) {
    [step]: notify(message="ok")
}
"#;
        let graph = GraphParser::parse(content).unwrap();
        let node = graph.node_map.get("loop").unwrap();
        let node_data = &graph.graph[*node];
        if let NodeType::Foreach { item, list, .. } = &node_data.node_type {
            assert_eq!(item, "item");
            assert_eq!(list, "input.items");
        } else {
            panic!("Expected Foreach node");
        }
    }

    #[test]
    fn test_assignment_block_parsing() {
        let content = r#"
[init]: count = 0, name = "Alice"
[next]: notify(message="done")
[init] -> [next]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let node = graph.node_map.get("init").unwrap();
        let node_data = &graph.graph[*node];
        if let NodeType::Task(action) = &node_data.node_type {
            assert_eq!(action.name, "set_context");
            assert_eq!(action.params.get("count").unwrap(), "0");
            assert_eq!(action.params.get("name").unwrap(), "\"Alice\"");
        } else {
            panic!("Expected Task node, got {:?}", node_data.node_type);
        }
    }

    #[test]
    fn test_assignment_single() {
        let content = r#"
[init]: result = $output.data
"#;
        let graph = GraphParser::parse(content).unwrap();
        let node = graph.node_map.get("init").unwrap();
        let node_data = &graph.graph[*node];
        if let NodeType::Task(action) = &node_data.node_type {
            assert_eq!(action.name, "set_context");
            assert_eq!(action.params.get("result").unwrap(), "$output.data");
        } else {
            panic!("Expected Task node");
        }
    }

    // ---- Triple-quoted strings ----

    #[test]
    fn test_triple_quoted_in_task_param() {
        let input = r#"
            [run]: bash(command="""echo "hello world" && echo '{"key":"value"}'""")
        "#;
        let wf = GraphParser::parse(input).unwrap();
        let node = &wf.graph[*wf.node_map.get("run").unwrap()];
        if let NodeType::Task(action) = &node.node_type {
            assert_eq!(action.name, "bash");
            let cmd = action.params.get("command").unwrap();
            assert!(cmd.contains(r#"echo "hello world""#));
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_triple_quoted_multiline_param() {
        let input = "[run]: bash(command=\"\"\"line1\nline2\nline3\"\"\")";
        let wf = GraphParser::parse(input).unwrap();
        assert!(wf.node_map.contains_key("run"));
    }

    #[test]
    fn test_triple_quoted_with_regular_string() {
        let input = r#"
            [a]: bash(command="""echo "test" done""")
            [b]: bash(command="echo simple")
        "#;
        let wf = GraphParser::parse(input).unwrap();
        assert!(wf.node_map.contains_key("a"));
        assert!(wf.node_map.contains_key("b"));
    }

    #[test]
    fn test_triple_quoted_assignment() {
        let input = r#"
            [setup]: cmd = """curl -H "Auth: key" https://api.com"""
        "#;
        let wf = GraphParser::parse(input).unwrap();
        assert!(wf.node_map.contains_key("setup"));
    }
}
