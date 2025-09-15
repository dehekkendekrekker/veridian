use crate::server::ProjectConfig;
use regex::Regex;
use ropey::Rope;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tower_lsp::lsp_types::*;
use walkdir::DirEntry;
use log::{debug, error};


pub fn get_diagnostics(
    uri: Url,
    rope: &Rope,
    #[allow(unused_variables)] files: Vec<Url>,
    conf: &ProjectConfig,
) -> PublishDiagnosticsParams {
    if !(cfg!(test) && (uri.to_string().starts_with("file:///test"))) {
        let mut diagnostics : Vec<Diagnostic> = Vec::new();
        if conf.verilator.syntax.enabled {
            diagnostics.extend(
                if let Ok(path) = uri.to_file_path() {
                    verilator_syntax(
                        rope,
                        path,
                        &conf.verilator.syntax.path,
                        &conf.verilator.syntax.args,
                        &conf.project_path,
                    )
                    .unwrap_or_default()
                } else {
                    error!("Path not ok: {:#?}", uri.to_file_path());
                    Vec::new()
                }
            );
        }


        if conf.verible.syntax.enabled {
            diagnostics.extend(
                verible_syntax(rope, &conf.verible.syntax.path, &conf.verible.syntax.args)
                    .unwrap_or_default());
        }
        if conf.verible.lint.enabled {
            diagnostics.extend(
                if let Ok(path) = uri.to_file_path() {
                    verible_lint(
                        rope,
                        path,
                        &conf.verible.lint.path,
                        &conf.verible.syntax.args,
                        &conf.project_path,
                    )
                    .unwrap_or_default()
                } else {
                    error!("Path not ok: {:#?}", uri.to_file_path());
                    Vec::new()
                }
            );
       }
        PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        }
    } else {
        PublishDiagnosticsParams {
            uri,
            diagnostics: Vec::new(),
            version: None,
        }
    }
}

pub fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

/// convert captured severity string to DiagnosticSeverity
fn verilator_severity(severity: &str) -> Option<DiagnosticSeverity> {
    match severity {
        "Error" => Some(DiagnosticSeverity::ERROR),
        s if s.starts_with("Warning") => Some(DiagnosticSeverity::WARNING),
        // NOTE: afaik, verilator doesn't have an info or hint severity
        _ => Some(DiagnosticSeverity::INFORMATION),
    }
}


fn verible_lint (
    rope: &Rope,
    file_path: PathBuf,
    binary_path: &String,
    args: &[String],
    cwd: &PathBuf
) -> Option<Vec<Diagnostic>> {
    let mut child = Command::new(binary_path)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .args(args)
        .arg(file_path.to_str()?)
        .spawn()
        .ok()?;


    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^.+:(?P<line>\d*):(?P<startcol>\d*)(?:-(?P<endcol>\d*))?:\s(?P<message>.*)\s.*$").unwrap()
    });
    // write file to stdin, read output from stdout
    rope.write_to(child.stdin.as_mut()?).ok()?;
    let output = child.wait_with_output().ok()?;

    debug!("Verible lint output: {:#?}", output);

    if !output.status.success() {
        let mut diags: Vec<Diagnostic> = Vec::new();
        let raw_output = String::from_utf8(output.stderr).ok()?;
        debug!("Lines: {:#?}", raw_output.lines());
        for error in raw_output.lines() {
            let caps = re.captures(error)?;
            let line: u32 = caps.name("line")?.as_str().parse().ok()?;
            let startcol: u32 = caps.name("startcol")?.as_str().parse().ok()?;
            let endcol: Option<u32> = match caps.name("endcol").map(|e| e.as_str().parse()) {
                Some(Ok(e)) => Some(e),
                None => None,
                Some(Err(_)) => return None,
            };
            let start_pos = Position::new(line - 1, startcol - 1);
            let end_pos = Position::new(line - 1, endcol.unwrap_or(startcol) - 1);
            diags.push(Diagnostic::new(
                Range::new(start_pos, end_pos),
                Some(DiagnosticSeverity::ERROR),
                None,
                Some("verible".to_string()),
                caps.name("message")?.as_str().to_string(),
                None,
                None,
            ));
        }
        Some(diags)
    } else {
        None
    }


}






/// syntax checking using verilator --lint-only
fn verilator_syntax(
    rope: &Rope,
    file_path: PathBuf,
    verilator_syntax_path: &str,
    verilator_syntax_args: &[String],
    project_path: &PathBuf
) -> Option<Vec<Diagnostic>> {

    let split_args: Vec<&str> = verilator_syntax_args
    .iter()
    .flat_map(|s| s.split_whitespace())
    .collect();

    debug!("Current working directory: {:?}", project_path); 
    let mut child = Command::new(verilator_syntax_path)
        .current_dir(project_path)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .args(split_args)
        .arg(file_path.to_str()?)
        .spawn()
        .ok()?;

    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"%(?P<severity>Error|Warning)(-(?P<warning_type>[A-Z0-9_]+))?: (?P<filepath>[^:]+):(?P<line>\d+):((?P<col>\d+):)? ?(?P<message>.*)",
        )
        .unwrap()
    });
    // write file to stdin, read output from stdout
    rope.write_to(child.stdin.as_mut()?).ok()?;
    let output = child.wait_with_output().ok()?;

    debug!("Verilator output: {:#?}", output);

    if !output.status.success() {
        let mut diags: Vec<Diagnostic> = Vec::new();
        let raw_output = String::from_utf8(output.stderr).ok()?;
        let filtered_output = raw_output
            .lines()
            .filter(|line| line.starts_with('%'))
            .collect::<Vec<&str>>();
        for error in filtered_output {
            let caps = match re.captures(error) {
                Some(caps) => caps,
                None => continue,
            };

            // check if diagnostic is for this file, since verilator can provide diagnostics for
            // included files
            if caps.name("filepath")?.as_str() != file_path.to_str().unwrap_or("") {
                continue;
            }
            let severity = verilator_severity(caps.name("severity")?.as_str());
            let line: u32 = caps.name("line")?.as_str().to_string().parse().ok()?;
            let col: u32 = caps.name("col").map_or("1", |m| m.as_str()).parse().ok()?;
            let pos = Position::new(line - 1, col - 1);
            let msg = match severity {
                Some(DiagnosticSeverity::ERROR) => caps.name("message")?.as_str().to_string(),
                Some(DiagnosticSeverity::WARNING) => format!(
                    "{}: {}",
                    caps.name("warning_type")?.as_str(),
                    caps.name("message")?.as_str()
                ),
                _ => "".to_string(),
            };
            diags.push(Diagnostic::new(
                Range::new(pos, pos),
                severity,
                None,
                Some("verilator".to_string()),
                msg,
                None,
                None,
            ));
        }
        Some(diags)
    } else {
        None
    }
}

/// syntax checking using verible-verilog-syntax
fn verible_syntax(
    rope: &Rope,
    verible_syntax_path: &str,
    verible_syntax_args: &[String],
) -> Option<Vec<Diagnostic>> {
    let mut child = Command::new(verible_syntax_path)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .args(verible_syntax_args)
        .arg("-")
        .spawn()
        .ok()?;

    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"^.+:(?P<line>\d*):(?P<startcol>\d*)(?:-(?P<endcol>\d*))?:\s(?P<message>.*)\s.*$",
        )
        .unwrap()
    });
    // write file to stdin, read output from stdout
    rope.write_to(child.stdin.as_mut()?).ok()?;
    let output = child.wait_with_output().ok()?;

    debug!("Verible output: {:#?}", output);

    if !output.status.success() {
        let mut diags: Vec<Diagnostic> = Vec::new();
        let raw_output = String::from_utf8(output.stdout).ok()?;
        for error in raw_output.lines() {
            let caps = re.captures(error)?;
            let line: u32 = caps.name("line")?.as_str().parse().ok()?;
            let startcol: u32 = caps.name("startcol")?.as_str().parse().ok()?;
            let endcol: Option<u32> = match caps.name("endcol").map(|e| e.as_str().parse()) {
                Some(Ok(e)) => Some(e),
                None => None,
                Some(Err(_)) => return None,
            };
            let start_pos = Position::new(line - 1, startcol - 1);
            let end_pos = Position::new(line - 1, endcol.unwrap_or(startcol) - 1);
            diags.push(Diagnostic::new(
                Range::new(start_pos, end_pos),
                Some(DiagnosticSeverity::ERROR),
                None,
                Some("verible".to_string()),
                caps.name("message")?.as_str().to_string(),
                None,
                None,
            ));
        }
        Some(diags)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::test_init;
    use std::fs::File;
    use std::io::Write;
    use tempdir::TempDir;

    #[test]
   #[test]
    fn test_unsaved_file() {
        test_init();
        let uri = Url::parse("file://test.sv").unwrap();
        get_diagnostics(
            uri.clone(),
            &Rope::default(),
            vec![uri],
            &ProjectConfig::default(),
        );
    }

    #[test]
    fn test_verible_syntax() {
        let text = r#"module test;
    logic abc;
    logic abcd;

  a
endmodule
"#;
        let doc = Rope::from_str(text);
        let errors = verible_syntax(&doc, "verible-verilog-syntax", &[])
            .expect("verible-verilog-syntax not found, test can not run");
        let expected: Vec<Diagnostic> = vec![Diagnostic {
            range: Range {
                start: Position {
                    line: 5,
                    character: 0,
                },
                end: Position {
                    line: 5,
                    character: 8,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            source: Some("verible".to_string()),
            message: "syntax error at token".to_string(),
            related_information: None,
            tags: None,
            code_description: None,
            data: None,
        }];
        assert_eq!(errors, expected);
    }

    #[test]
    fn test_verilator_syntax() {
        let text = r#"module test;
    logic abc;
    logic abcd;

  a
endmodule
"#;
        let doc = Rope::from_str(text);

        // verilator can't read from stdin so we must create a temp dir to place our
        // test file
        let dir = TempDir::new("verilator_test").unwrap();
        let file_path_1 = dir.path().join("test.sv");
        let mut f = File::create(&file_path_1).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        f.sync_all().unwrap();

        let errors = verilator_syntax(
            &doc,
            file_path_1,
            "verilator",
            &[
                "--lint-only".to_string(),
                "--sv".to_string(),
                "-Wall".to_string(),
            ],
        )
        .expect("verilator not found, test can not run");

        drop(f);
        dir.close().unwrap();

        let expected: Vec<Diagnostic> = vec![Diagnostic {
            range: Range {
                start: Position {
                    line: 5,
                    character: 0,
                },
                end: Position {
                    line: 5,
                    character: 0,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            source: Some("verilator".to_string()),
            message: "syntax error, unexpected endmodule, expecting IDENTIFIER or randomize"
                .to_string(),
            related_information: None,
            tags: None,
            code_description: None,
            data: None,
        }];
        assert_eq!(errors[0].severity, expected[0].severity);
        assert_eq!(errors[0].range.start.line, expected[0].range.start.line);
        assert_eq!(errors[0].range.end.line, expected[0].range.end.line);
        assert!(errors[0].message.contains("syntax error"));
    }
}
