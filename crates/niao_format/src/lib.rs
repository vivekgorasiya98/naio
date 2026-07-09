use niao_ast::*;
use niao_parser::parse;

#[derive(Debug)]
pub enum FormatError {
    Parse(niao_parser::ParseError),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<niao_parser::ParseError> for FormatError {
    fn from(e: niao_parser::ParseError) -> Self {
        FormatError::Parse(e)
    }
}

pub fn format_source(source: &str) -> Result<String, FormatError> {
    let program = parse(source)?;
    Ok(format_program(&program))
}

fn format_program(program: &Program) -> String {
    let mut out = String::new();
    for (i, item) in program.items.iter().enumerate() {
        if i > 0 {
            // Consecutive top-level statements stay together; other items get a blank line.
            let both_stmts = matches!(item, TopLevel::Stmt(_))
                && matches!(program.items[i - 1], TopLevel::Stmt(_));
            if !both_stmts {
                out.push('\n');
            }
        }
        out.push_str(&format_top_level(item));
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn format_top_level(item: &TopLevel) -> String {
    match item {
        TopLevel::Import(imp) => format!("import \"{}\";\n", imp.path),
        TopLevel::Fn(f) => format_fn(f),
        TopLevel::Struct(s) => format_struct(s),
        TopLevel::Server(s) => format_server(s),
        TopLevel::Route(r) => format_route(r),
        TopLevel::Stmt(s) => format!("{}\n", format_stmt(s, 0)),
        TopLevel::Class(c) => format_class(c),
        TopLevel::Trait(t) => format_trait(t),
    }
}

fn vis_prefix(vis: Visibility) -> &'static str {
    match vis {
        Visibility::Private => "private ",
        Visibility::Public => "",
    }
}

fn format_class(c: &ClassDef) -> String {
    let mut out = format!("class {}", c.name);
    if let Some(parent) = &c.extends {
        out.push_str(&format!(" extends {parent}"));
    }
    if !c.implements.is_empty() {
        out.push_str(" implements ");
        out.push_str(&c.implements.join(", "));
    }
    out.push_str(" {\n");
    for member in &c.members {
        match member {
            ClassMember::Field {
                name,
                ty,
                visibility,
                ..
            } => {
                out.push_str(&format!(
                    "    {}{}: {};\n",
                    vis_prefix(*visibility),
                    name,
                    format_type(ty)
                ));
            }
            ClassMember::Method { def, visibility } => {
                out.push_str(&format!(
                    "    {}{}\n",
                    vis_prefix(*visibility),
                    format_fn_body(def, 1)
                ));
            }
            ClassMember::StaticMethod { def, visibility } => {
                out.push_str(&format!(
                    "    {}static {}\n",
                    vis_prefix(*visibility),
                    format_fn_body(def, 1)
                ));
            }
            ClassMember::StaticField {
                name,
                init,
                visibility,
                ..
            } => {
                out.push_str(&format!("    {}static let {}", vis_prefix(*visibility), name));
                if let Some(expr) = init {
                    out.push_str(&format!(" = {}", format_expr(expr)));
                }
                out.push_str(";\n");
            }
        }
    }
    out.push('}');
    out
}

fn format_trait(t: &TraitDef) -> String {
    let mut out = format!("trait {} {{\n", t.name);
    for sig in &t.methods {
        out.push_str(&format!("    {}\n", format_method_sig(sig)));
    }
    out.push('}');
    out
}

fn format_method_sig(sig: &MethodSig) -> String {
    let mut out = format!("fn {}", sig.name);
    out.push('(');
    for (i, p) in sig.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&p.name);
        if let Some(ty) = &p.ty {
            out.push_str(&format!(": {}", format_type(ty)));
        }
    }
    out.push(')');
    if let Some(ty) = &sig.return_type {
        out.push_str(&format!(" -> {}", format_type(ty)));
    }
    out
}

fn format_fn_body(f: &FnDef, indent: usize) -> String {
    let mut out = format!("fn {}", f.name);
    out.push('(');
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&p.name);
        if let Some(ty) = &p.ty {
            out.push_str(&format!(": {}", format_type(ty)));
        }
    }
    out.push(')');
    if let Some(ty) = &f.return_type {
        out.push_str(&format!(" -> {}", format_type(ty)));
    }
    out.push(' ');
    out.push_str(&format_block(&f.body, indent));
    out
}

fn format_fn(f: &FnDef) -> String {
    let mut out = format!("fn {}", f.name);
    out.push('(');
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&p.name);
        if let Some(ty) = &p.ty {
            out.push_str(&format!(": {}", format_type(ty)));
        }
    }
    out.push(')');
    if let Some(ty) = &f.return_type {
        out.push_str(&format!(" -> {}", format_type(ty)));
    }
    out.push(' ');
    out.push_str(&format_block(&f.body, 0));
    out
}

fn format_struct(s: &StructDef) -> String {
    let mut out = format!("struct {} {{\n", s.name);
    for field in &s.fields {
        out.push_str(&format!("    {}: {};\n", field.name, format_type(&field.ty)));
    }
    out.push('}');
    out
}

fn format_server(s: &ServerBlock) -> String {
    let mut out = "server {\n".to_string();
    for field in &s.fields {
        out.push_str(&format!("    {} = {};\n", field.name, format_expr(&field.value)));
    }
    out.push('}');
    out
}

fn format_route(r: &RouteBlock) -> String {
    let method = match r.method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Patch => "PATCH",
    };
    format!(
        "{} \"{}\" {}",
        method,
        r.path,
        format_block(&r.body, 0)
    )
}

fn format_block(block: &Block, indent: usize) -> String {
    let pad = "    ".repeat(indent);
    let inner = "    ".repeat(indent + 1);
    let mut out = "{\n".to_string();
    for stmt in &block.stmts {
        out.push_str(&format!("{}{}\n", inner, format_stmt(stmt, indent + 1)));
    }
    out.push_str(&format!("{pad}}}"));
    out
}

fn format_stmt(stmt: &Stmt, indent: usize) -> String {
    match stmt {
        Stmt::VarDecl { name, ty, init, .. } => {
            let mut out = format!("let {name}");
            if let Some(t) = ty {
                out.push_str(&format!(": {}", format_type(t)));
            }
            if let Some(expr) = init {
                out.push_str(&format!(" = {}", format_expr(expr)));
            }
            out.push(';');
            out
        }
        Stmt::Assign { target, op, value, .. } => {
            let target_str = match target {
                AssignTarget::Name(n) => n.clone(),
                AssignTarget::Member { object, field } => {
                    format!("{}.{}", format_expr(object), field)
                }
                AssignTarget::Index { object, index } => {
                    format!("{}[{}]", format_expr(object), format_expr(index))
                }
            };
            let op_str = match op {
                AssignOp::Assign => "=",
                AssignOp::AddAssign => "+=",
                AssignOp::SubAssign => "-=",
            };
            format!("{target_str} {op_str} {};", format_expr(value))
        }
        Stmt::Expr(expr) => {
            if matches!(expr, Expr::Nil(_)) {
                return "{}".into();
            }
            format!("{};", format_expr(expr))
        }
        Stmt::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            let mut out = format!("if {} {}", format_expr(cond), format_block(then_block, indent));
            if let Some(else_blk) = else_block {
                out.push_str(" else ");
                out.push_str(&format_block(else_blk, indent));
            }
            out
        }
        Stmt::While { cond, body, .. } => {
            format!(
                "while {} {}",
                format_expr(cond),
                format_block(body, indent)
            )
        }
        Stmt::For { var, iter, body, .. } => {
            format!(
                "for {} in {} {}",
                var,
                format_expr(iter),
                format_block(body, indent)
            )
        }
        Stmt::Return { value, .. } => {
            if let Some(expr) = value {
                format!("return {};", format_expr(expr))
            } else {
                "return;".into()
            }
        }
        Stmt::Try {
            try_block,
            catch_var,
            catch_block,
            ..
        } => {
            format!(
                "try {} catch ({}) {}",
                format_block(try_block, indent),
                catch_var,
                format_block(catch_block, indent)
            )
        }
        Stmt::Break(_) => "break;".into(),
        Stmt::Continue(_) => "continue;".into(),
        Stmt::Throw { value, .. } => format!("throw {};", format_expr(value)),
    }
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Int(v, _) => v.to_string(),
        Expr::Float(v, _) => v.to_string(),
        Expr::String(s, _) => format!("\"{s}\""),
        Expr::Bool(v, _) => v.to_string(),
        Expr::Nil(_) => "nil".into(),
        Expr::Ident(name, _) => name.clone(),
        Expr::Binary { left, op, right, .. } => {
            format!(
                "{} {} {}",
                format_expr(left),
                format_binop(*op),
                format_expr(right)
            )
        }
        Expr::Unary { op, expr, .. } => {
            format!("{}{}", format_unaryop(*op), format_expr(expr))
        }
        Expr::Call { callee, args, .. } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("{}({})", format_expr(callee), arg_strs.join(", "))
        }
        Expr::Member { object, field, .. } => format!("{}.{}", format_expr(object), field),
        Expr::Index { object, index, .. } => {
            format!("{}[{}]", format_expr(object), format_expr(index))
        }
        Expr::Array { elements, .. } => {
            let parts: Vec<String> = elements.iter().map(format_expr).collect();
            format!("[{}]", parts.join(", "))
        }
        Expr::Object { fields, .. } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", format_expr(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Expr::StructInit { name, fields, .. } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", format_expr(v)))
                .collect();
            format!("{} {{ {} }}", name, parts.join(", "))
        }
        Expr::ClassInit { name, fields, .. } => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", format_expr(v)))
                .collect();
            format!("{} {{ {} }}", name, parts.join(", "))
        }
        Expr::SuperCall { method, args, .. } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("super.{}({})", method, arg_strs.join(", "))
        }
    }
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

fn format_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::FloorDiv => "//",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn format_unaryop(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::Neg => "-",
    }
}
