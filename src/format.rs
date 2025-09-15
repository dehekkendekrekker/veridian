use crate::server::LSPServer;
use log::info;
use tower_lsp::lsp_types::*;

impl LSPServer {
    pub fn formatting(&self, params: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let uri = params.text_document.uri;
        info!("formatting {}", &uri);
        let file_id = self.srcs.get_id(&uri).to_owned();
        self.srcs.wait_parse_ready(file_id, false);

        None
   }

    pub fn range_formatting(&self, params: DocumentRangeFormattingParams) -> Option<Vec<TextEdit>> {
        let uri = params.text_document.uri;
        info!("range formatting {}", &uri);
        let file_id = self.srcs.get_id(&uri).to_owned();
        self.srcs.wait_parse_ready(file_id, false);


        None
   }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::ProjectConfig;
    use crate::support::test_init;
    use which::which;

    #[test]
    fn test_formatting() {
        test_init();
        let text = r#"
module test;
  logic a;
   logic b;
endmodule"#;
        let text_fixed = r#"
module test;
  logic a;
  logic b;
endmodule
"#;
        let doc = Rope::from_str(text);
        if which("verible-verilog-format").is_ok() {
            assert_eq!(
                format_document(
                    &doc,
                    None,
                    &ProjectConfig::default().verible.format.path,
                    &[]
                )
                .unwrap(),
                text_fixed.to_string()
            );
        }
    }

    #[test]
    fn test_range_formatting() {
        test_init();
        let text = r#"module t1;
    logic a;
 logic b;
         logic c;
endmodule


module t2;
    logic a;
 logic b;
         logic c;
endmodule"#;

        let text_fixed = r#"module t1;
  logic a;
  logic b;
  logic c;
endmodule


module t2;
    logic a;
 logic b;
         logic c;
endmodule
"#;
        let doc = Rope::from_str(text);
        if which("verible-verilog-format").is_ok() {
            assert_eq!(
                format_document(
                    &doc,
                    Some(Range::new(Position::new(0, 0), Position::new(4, 9))),
                    &ProjectConfig::default().verible.format.path,
                    &[]
                )
                .unwrap(),
                text_fixed.to_string()
            );
        }
    }
}
