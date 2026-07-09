use niao_ast::*;
use niao_parser::parse;
use std::path::Path;

#[derive(Debug)]
pub enum DocsError {
    Parse(niao_parser::ParseError),
}

impl std::fmt::Display for DocsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocsError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl std::error::Error for DocsError {}

impl From<niao_parser::ParseError> for DocsError {
    fn from(e: niao_parser::ParseError) -> Self {
        DocsError::Parse(e)
    }
}

pub fn generate_docs(source: &str, file: &Path) -> Result<String, DocsError> {
    let program = parse(source)?;
    let title = file
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Ok(render_html(&title, &program))
}

fn render_html(title: &str, program: &Program) -> String {
    let mut body = String::new();
    for item in &program.items {
        match item {
            TopLevel::Fn(f) => {
                body.push_str(&format!("<section class=\"fn\">"));
                body.push_str(&format!("<h2>fn {}</h2>", f.name));
                if !f.params.is_empty() {
                    body.push_str("<h3>Parameters</h3><ul>");
                    for p in &f.params {
                        let ty = p
                            .ty
                            .as_ref()
                            .map(format_type)
                            .unwrap_or_else(|| "any".into());
                        body.push_str(&format!("<li><code>{}</code>: {}</li>", p.name, ty));
                    }
                    body.push_str("</ul>");
                }
                if let Some(ret) = &f.return_type {
                    body.push_str(&format!(
                        "<p>Returns: <code>{}</code></p>",
                        format_type(ret)
                    ));
                }
                body.push_str("</section>");
            }
            TopLevel::Struct(s) => {
                body.push_str(&format!("<section class=\"struct\">"));
                body.push_str(&format!("<h2>struct {}</h2><ul>", s.name));
                for field in &s.fields {
                    body.push_str(&format!(
                        "<li><code>{}</code>: {}</li>",
                        field.name,
                        format_type(&field.ty)
                    ));
                }
                body.push_str("</ul></section>");
            }
            TopLevel::Class(c) => {
                body.push_str("<section class=\"class\">");
                body.push_str(&format!("<h2>class {}</h2>", c.name));
                if let Some(parent) = &c.extends {
                    body.push_str(&format!("<p>extends <code>{parent}</code></p>"));
                }
                if !c.implements.is_empty() {
                    body.push_str(&format!(
                        "<p>implements {}</p>",
                        c.implements
                            .iter()
                            .map(|t| format!("<code>{t}</code>"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                body.push_str("<ul>");
                for member in &c.members {
                    match member {
                        ClassMember::Field { name, ty, .. } => {
                            body.push_str(&format!(
                                "<li><code>{}</code>: {}</li>",
                                name,
                                format_type(ty)
                            ));
                        }
                        ClassMember::Method { def, .. } | ClassMember::StaticMethod { def, .. } => {
                            body.push_str(&format!("<li><code>fn {}</code></li>", def.name));
                        }
                        ClassMember::StaticField { name, .. } => {
                            body.push_str(&format!("<li><code>static let {}</code></li>", name));
                        }
                    }
                }
                body.push_str("</ul></section>");
            }
            TopLevel::Trait(t) => {
                body.push_str("<section class=\"trait\">");
                body.push_str(&format!("<h2>trait {}</h2><ul>", t.name));
                for sig in &t.methods {
                    body.push_str(&format!("<li><code>fn {}</code></li>", sig.name));
                }
                body.push_str("</ul></section>");
            }
            _ => {}
        }
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>{title} - Niao Docs</title>
  <style>
    body {{ font-family: system-ui, sans-serif; max-width: 800px; margin: 2rem auto; padding: 0 1rem; }}
    h1 {{ color: #333; }}
    h2 {{ color: #555; border-bottom: 1px solid #ddd; padding-bottom: 0.25rem; }}
    code {{ background: #f4f4f4; padding: 0.1rem 0.3rem; border-radius: 3px; }}
    section {{ margin-bottom: 2rem; }}
  </style>
</head>
<body>
  <h1>{title}</h1>
  {body}
</body>
</html>"#
    )
}

fn format_type(ty: &TypeName) -> String {
    match ty {
        TypeName::Int => "int".into(),
        TypeName::Float => "float".into(),
        TypeName::String => "string".into(),
        TypeName::Bool => "bool".into(),
        TypeName::Void => "void".into(),
        TypeName::Array => "array".into(),
        TypeName::Error => "error".into(),
        TypeName::Named(n) => n.clone(),
    }
}
