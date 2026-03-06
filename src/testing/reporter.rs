// src/testing/reporter.rs
//
// Output formatters for `juglans test` results.

use serde_json::json;

use super::FileTestResult;

/// Print test results in human-readable text format
pub fn print_text(results: &[FileTestResult]) {
    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut total_duration = std::time::Duration::ZERO;

    for file_result in results {
        if file_result.results.is_empty() {
            continue;
        }

        println!("\n  {}", file_result.path.display());

        for result in &file_result.results {
            let icon = if result.passed {
                "\x1b[32m✓\x1b[0m"
            } else {
                "\x1b[31m✗\x1b[0m"
            };
            let duration = format!("{:.1}s", result.duration.as_secs_f64());
            let assertions = if result.assertions > 0 {
                format!("  {} assertion(s)", result.assertions)
            } else {
                String::new()
            };

            println!(
                "    {} {:<40} {:>6}{}",
                icon, result.name, duration, assertions,
            );

            if !result.failed_assertions.is_empty() {
                for msg in &result.failed_assertions {
                    let display_msg = msg.lines().next().unwrap_or(msg).trim();
                    println!("      \x1b[31m{}\x1b[0m", display_msg);
                }
            } else if let Some(error) = &result.error {
                // Non-assertion error (e.g., tool execution failure)
                let display_error = error.lines().next().unwrap_or(error).trim();
                println!("      \x1b[31m{}\x1b[0m", display_error);
            }

            if result.passed {
                total_passed += 1;
            } else {
                total_failed += 1;
            }
            total_duration += result.duration;
        }
    }

    // Summary line
    println!();
    if total_failed > 0 {
        println!(
            "  \x1b[32m{} passed\x1b[0m, \x1b[31m{} failed\x1b[0m | {:.1}s",
            total_passed,
            total_failed,
            total_duration.as_secs_f64(),
        );
    } else if total_passed > 0 {
        println!(
            "  \x1b[32m{} passed\x1b[0m | {:.1}s",
            total_passed,
            total_duration.as_secs_f64(),
        );
    } else {
        println!("  No tests found.");
    }
    println!();
}

/// Print test results in JSON format (for CI)
pub fn print_json(results: &[FileTestResult]) {
    let json_results: Vec<_> = results
        .iter()
        .flat_map(|file_result| {
            file_result.results.iter().map(move |r| {
                json!({
                    "file": file_result.path.display().to_string(),
                    "name": r.name,
                    "passed": r.passed,
                    "duration_ms": r.duration.as_millis(),
                    "assertions": r.assertions,
                    "error": r.error,
                    "failed_assertions": r.failed_assertions,
                })
            })
        })
        .collect();

    let total_passed: usize = results.iter().map(|f| f.passed_count()).sum();
    let total_failed: usize = results.iter().map(|f| f.failed_count()).sum();

    let output = json!({
        "passed": total_passed,
        "failed": total_failed,
        "total": total_passed + total_failed,
        "results": json_results,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

/// Print test results in JUnit XML format (for GitHub Actions)
pub fn print_junit(results: &[FileTestResult]) {
    let mut total_tests = 0;
    let mut total_failures = 0;
    let mut total_time = 0.0f64;
    let mut testcases = String::new();

    for file_result in results {
        let testsuite_name = file_result.path.display().to_string();

        for result in &file_result.results {
            total_tests += 1;
            total_time += result.duration.as_secs_f64();

            if result.passed {
                testcases.push_str(&format!(
                    "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\" />\n",
                    testsuite_name,
                    result.name,
                    result.duration.as_secs_f64()
                ));
            } else {
                total_failures += 1;
                let error_msg = result
                    .error
                    .as_deref()
                    .unwrap_or("unknown error")
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;");
                testcases.push_str(&format!(
                    "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">\n      <failure message=\"{}\" />\n    </testcase>\n",
                    testsuite_name, result.name, result.duration.as_secs_f64(), error_msg
                ));
            }
        }
    }

    println!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="juglans test" tests="{}" failures="{}" time="{:.3}">
{}  </testsuite>"#,
        total_tests, total_failures, total_time, testcases
    );
}
