use niao_ast::*;
use niao_lexer::{Token, TokenKind};
use std::collections::HashSet;

pub use niao_errors::ParseError;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    class_names: HashSet<String>,
    struct_names: HashSet<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            class_names: HashSet::new(),
            struct_names: HashSet::new(),
        }
    }

    pub fn parse(&mut self) -> Result<Program, ParseError> {
        let start = self.current_span();
        let mut items = Vec::new();
        while !self.is_at_end() {
            items.push(self.parse_top_level()?);
        }
        Ok(Program {
            items,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_top_level(&mut self) -> Result<TopLevel, ParseError> {
        let item = match &self.peek().kind {
            TokenKind::Import => TopLevel::Import(self.parse_import()?),
            TokenKind::Fn => TopLevel::Fn(self.parse_fn_def()?),
            TokenKind::Struct => {
                let s = self.parse_struct_def()?;
                self.struct_names.insert(s.name.clone());
                TopLevel::Struct(s)
            }
            TokenKind::Class => {
                let c = self.parse_class_def()?;
                self.class_names.insert(c.name.clone());
                TopLevel::Class(c)
            }
            TokenKind::Trait => TopLevel::Trait(self.parse_trait_def()?),
            TokenKind::Server => TopLevel::Server(self.parse_server_block()?),
            TokenKind::Get | TokenKind::Post | TokenKind::Put | TokenKind::Delete | TokenKind::Patch => {
                TopLevel::Route(self.parse_route_block()?)
            }
            _ => TopLevel::Stmt(self.parse_stmt()?),
        };
        Ok(item)
    }

    fn parse_import(&mut self) -> Result<ImportStmt, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Import)?;
        let path = match self.advance().kind.clone() {
            TokenKind::String(s) => s,
            _ => return Err(self.error("string literal")),
        };
        let alias = if self.check(&TokenKind::As) {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.optional_semicolon();
        Ok(ImportStmt {
            path,
            alias,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_fn_def(&mut self) -> Result<FnDef, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;
        let return_type = if self.check(&TokenKind::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        let body_span = body.span;
        Ok(FnDef {
            name,
            params,
            return_type,
            body,
            span: Span::merge(start, body_span),
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let start = self.current_span();
                let name = self.expect_ident()?;
                let ty = if self.check(&TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(Param {
                    name,
                    ty,
                    span: Span::merge(start, self.previous().span),
                });
                if !self.match_kind(&TokenKind::Comma) {
                    break;
                }
            }
        }
        Ok(params)
    }

    fn parse_struct_def(&mut self) -> Result<StructDef, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            let fstart = self.current_span();
            let fname = self.expect_ident()?;
            self.expect(TokenKind::Colon)?;
            let fty = self.parse_type()?;
            fields.push(FieldDef {
                name: fname,
                ty: fty,
                span: Span::merge(fstart, self.previous().span),
            });
        }
        self.expect(TokenKind::RBrace)?;
        Ok(StructDef {
            name,
            fields,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_class_def(&mut self) -> Result<ClassDef, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Class)?;
        let name = self.expect_ident()?;
        let mut extends = None;
        let mut implements = Vec::new();
        if self.match_kind(&TokenKind::Extends) {
            extends = Some(self.expect_ident()?);
        }
        while self.match_kind(&TokenKind::Implements) {
            implements.push(self.expect_ident()?);
            while self.match_kind(&TokenKind::Comma) {
                implements.push(self.expect_ident()?);
            }
        }
        self.expect(TokenKind::LBrace)?;
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            members.push(self.parse_class_member()?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ClassDef {
            name,
            extends,
            implements,
            members,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_class_member(&mut self) -> Result<ClassMember, ParseError> {
        let start = self.current_span();
        let visibility = self.parse_visibility();
        if self.match_kind(&TokenKind::Static) {
            if self.check(&TokenKind::Fn) {
                let def = self.parse_fn_def()?;
                return Ok(ClassMember::StaticMethod { def, visibility });
            }
            self.expect(TokenKind::Let)?;
            let name = self.expect_ident()?;
            let init = if self.check(&TokenKind::Assign) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.optional_semicolon();
            return Ok(ClassMember::StaticField {
                name,
                init,
                visibility,
                span: Span::merge(start, self.previous().span),
            });
        }
        if self.check(&TokenKind::Fn) {
            let def = self.parse_fn_def()?;
            return Ok(ClassMember::Method { def, visibility });
        }
        let fname = self.expect_ident()?;
        self.expect(TokenKind::Colon)?;
        let fty = self.parse_type()?;
        self.optional_semicolon();
        Ok(ClassMember::Field {
            name: fname,
            ty: fty,
            visibility,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_visibility(&mut self) -> Visibility {
        if self.match_kind(&TokenKind::Private) {
            Visibility::Private
        } else if self.match_kind(&TokenKind::Public) {
            Visibility::Public
        } else {
            Visibility::Public
        }
    }

    fn parse_trait_def(&mut self) -> Result<TraitDef, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Trait)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            methods.push(self.parse_method_sig()?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(TraitDef {
            name,
            methods,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_method_sig(&mut self) -> Result<MethodSig, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;
        let return_type = if self.check(&TokenKind::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.optional_semicolon();
        Ok(MethodSig {
            name,
            params,
            return_type,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_server_block(&mut self) -> Result<ServerBlock, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::Server)?;
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            let fstart = self.current_span();
            let name = self.expect_ident()?;
            self.expect(TokenKind::Assign)?;
            let value = self.parse_expr()?;
            self.optional_semicolon();
            fields.push(ServerField {
                name,
                value,
                span: Span::merge(fstart, self.previous().span),
            });
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ServerBlock {
            fields,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_route_block(&mut self) -> Result<RouteBlock, ParseError> {
        let start = self.current_span();
        let method = match self.advance().kind.clone() {
            TokenKind::Get => HttpMethod::Get,
            TokenKind::Post => HttpMethod::Post,
            TokenKind::Put => HttpMethod::Put,
            TokenKind::Delete => HttpMethod::Delete,
            TokenKind::Patch => HttpMethod::Patch,
            _ => return Err(self.error("HTTP method")),
        };
        let path = match self.advance().kind.clone() {
            TokenKind::String(s) => s,
            _ => return Err(self.error("route path string")),
        };
        let body = self.parse_block()?;
        let body_span = body.span;
        Ok(RouteBlock {
            method,
            path,
            body,
            span: Span::merge(start, body_span),
        })
    }

    fn optional_semicolon(&mut self) {
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let start = self.current_span();
        self.expect(TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(Block {
            stmts,
            span: Span::merge(start, self.previous().span),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_span();
        match &self.peek().kind {
            TokenKind::Let => {
                self.advance();
                let name = self.expect_ident()?;
                let ty = if self.check(&TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                let init = if self.check(&TokenKind::Assign) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.optional_semicolon();
                Ok(Stmt::VarDecl {
                    name,
                    ty,
                    init,
                    span: Span::merge(start, self.previous().span),
                })
            }
            TokenKind::If => {
                self.advance();
                let cond = self.parse_expr()?;
                let then_block = self.parse_block()?;
                let else_block = if self.match_kind(&TokenKind::Else) {
                    if self.check(&TokenKind::If) {
                        let else_stmt = self.parse_stmt()?;
                        match else_stmt {
                            Stmt::If {
                                cond,
                                then_block,
                                else_block,
                                span,
                            } => Some(Block {
                                stmts: vec![Stmt::If {
                                    cond,
                                    then_block,
                                    else_block,
                                    span,
                                }],
                                span,
                            }),
                            other => Some(Block {
                                stmts: vec![other],
                                span: Span::merge(start, self.previous().span),
                            }),
                        }
                    } else {
                        Some(self.parse_block()?)
                    }
                } else {
                    None
                };
                Ok(Stmt::If {
                    cond,
                    then_block,
                    else_block,
                    span: Span::merge(start, self.previous().span),
                })
            }
            TokenKind::While => {
                self.advance();
                let cond = self.parse_expr()?;
                let body = self.parse_block()?;
                let body_span = body.span;
                Ok(Stmt::While {
                    cond,
                    body,
                    span: Span::merge(start, body_span),
                })
            }
            TokenKind::For => {
                self.advance();
                let var = self.expect_ident()?;
                self.expect(TokenKind::In)?;
                let iter = self.parse_expr()?;
                let body = self.parse_block()?;
                let body_span = body.span;
                Ok(Stmt::For {
                    var,
                    iter,
                    body,
                    span: Span::merge(start, body_span),
                })
            }
            TokenKind::Return => {
                self.advance();
                let value = if self.check(&TokenKind::Semicolon)
                    || self.check(&TokenKind::RBrace)
                    || self.is_at_end()
                {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.optional_semicolon();
                Ok(Stmt::Return {
                    value,
                    span: Span::merge(start, self.previous().span),
                })
            }
            TokenKind::Try => {
                self.advance();
                let try_block = self.parse_block()?;
                self.expect(TokenKind::Catch)?;
                self.expect(TokenKind::LParen)?;
                let catch_var = self.expect_ident()?;
                self.expect(TokenKind::RParen)?;
                let catch_block = self.parse_block()?;
                let catch_span = catch_block.span;
                Ok(Stmt::Try {
                    try_block,
                    catch_var,
                    catch_block,
                    span: Span::merge(start, catch_span),
                })
            }
            TokenKind::Break => {
                self.advance();
                self.optional_semicolon();
                Ok(Stmt::Break(Span::merge(start, self.previous().span)))
            }
            TokenKind::Continue => {
                self.advance();
                self.optional_semicolon();
                Ok(Stmt::Continue(Span::merge(start, self.previous().span)))
            }
            TokenKind::Throw => {
                self.advance();
                let value = self.parse_expr()?;
                self.optional_semicolon();
                Ok(Stmt::Throw {
                    value,
                    span: Span::merge(start, self.previous().span),
                })
            }
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                Ok(Stmt::Expr(Expr::Nil(block.span)))
            }
            _ => {
                if let TokenKind::Ident(name) = self.peek().kind.clone() {
                    let next_is_assign = self.pos + 1 < self.tokens.len()
                        && matches!(self.tokens[self.pos + 1].kind, TokenKind::Assign | TokenKind::AddAssign | TokenKind::SubAssign);
                    if next_is_assign {
                        let start = self.current_span();
                        self.advance();
                        let op = match self.advance().kind {
                            TokenKind::Assign => AssignOp::Assign,
                            TokenKind::AddAssign => AssignOp::AddAssign,
                            TokenKind::SubAssign => AssignOp::SubAssign,
                            _ => unreachable!(),
                        };
                        let value = self.parse_expr()?;
                        self.optional_semicolon();
                        return Ok(Stmt::Assign {
                            target: AssignTarget::Name(name),
                            op,
                            value,
                            span: Span::merge(start, self.previous().span),
                        });
                    }
                }
                let expr = self.parse_expr()?;
                if self.check(&TokenKind::Assign)
                    || self.check(&TokenKind::AddAssign)
                    || self.check(&TokenKind::SubAssign)
                {
                    let target = match &expr {
                        Expr::Ident(name, _) => AssignTarget::Name(name.clone()),
                        Expr::Index { object, index, .. } => AssignTarget::Index {
                            object: object.clone(),
                            index: index.clone(),
                        },
                        Expr::Member { object, field, .. } => AssignTarget::Member {
                            object: object.clone(),
                            field: field.clone(),
                        },
                        _ => {
                            return Err(ParseError::Unexpected {
                                found: "invalid assignment target".into(),
                                expected: "assignable expression".into(),
                                line: self.current_span().line,
                                col: self.current_span().col,
                            });
                        }
                    };
                    let op = match self.advance().kind {
                        TokenKind::Assign => AssignOp::Assign,
                        TokenKind::AddAssign => AssignOp::AddAssign,
                        TokenKind::SubAssign => AssignOp::SubAssign,
                        _ => unreachable!(),
                    };
                    let value = self.parse_expr()?;
                    self.optional_semicolon();
                    return Ok(Stmt::Assign {
                        target,
                        op,
                        value,
                        span: Span::merge(expr.span(), self.previous().span),
                    });
                }
                self.optional_semicolon();
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_type(&mut self) -> Result<TypeName, ParseError> {
        Ok(match self.advance().kind.clone() {
            TokenKind::TypeInt => TypeName::Int,
            TokenKind::TypeFloat => TypeName::Float,
            TokenKind::TypeString => TypeName::String,
            TokenKind::TypeBool => TypeName::Bool,
            TokenKind::TypeVoid => TypeName::Void,
            TokenKind::TypeArray => TypeName::Array,
            TokenKind::Ident(name) if name == "error" => TypeName::Error,
            TokenKind::Ident(name) => TypeName::Named(name),
            other => {
                return Err(ParseError::Unexpected {
                    found: format!("{other:?}"),
                    expected: "type".into(),
                    line: self.previous().span.line,
                    col: self.previous().span.col,
                });
            }
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.match_kind(&TokenKind::Or) {
            let op = BinOp::Or;
            let right = self.parse_and()?;
            let span = Span::merge(left.span(), right.span());
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality()?;
        while self.match_kind(&TokenKind::And) {
            let op = BinOp::And;
            let right = self.parse_equality()?;
            let span = Span::merge(left.span(), right.span());
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = if self.match_kind(&TokenKind::Eq) {
                BinOp::Eq
            } else if self.match_kind(&TokenKind::Ne) {
                BinOp::Ne
            } else {
                break;
            };
            let right = self.parse_comparison()?;
            let span = Span::merge(left.span(), right.span());
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;
        loop {
            let op = if self.match_kind(&TokenKind::Lt) {
                BinOp::Lt
            } else if self.match_kind(&TokenKind::Gt) {
                BinOp::Gt
            } else if self.match_kind(&TokenKind::Le) {
                BinOp::Le
            } else if self.match_kind(&TokenKind::Ge) {
                BinOp::Ge
            } else {
                break;
            };
            let right = self.parse_term()?;
            let span = Span::merge(left.span(), right.span());
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_factor()?;
        loop {
            let op = if self.match_kind(&TokenKind::Plus) {
                BinOp::Add
            } else if self.match_kind(&TokenKind::Minus) {
                BinOp::Sub
            } else {
                break;
            };
            let right = self.parse_factor()?;
            let span = Span::merge(left.span(), right.span());
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = if self.match_kind(&TokenKind::Star) {
                BinOp::Mul
            } else if self.match_kind(&TokenKind::Slash) {
                BinOp::Div
            } else if self.match_kind(&TokenKind::FloorDiv) {
                BinOp::FloorDiv
            } else if self.match_kind(&TokenKind::Percent) {
                BinOp::Mod
            } else {
                break;
            };
            let right = self.parse_unary()?;
            let span = Span::merge(left.span(), right.span());
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        if self.match_kind(&TokenKind::Not) {
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
                span: Span::merge(start, self.previous().span),
            });
        }
        if self.match_kind(&TokenKind::Minus) {
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
                span: Span::merge(start, self.previous().span),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.match_kind(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                let span = Span::merge(expr.span(), self.previous().span);
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    span,
                };
            } else if self.match_kind(&TokenKind::Dot) {
                let field = self.expect_ident()?;
                let span = Span::merge(expr.span(), self.previous().span);
                if matches!(expr, Expr::Ident(ref name, _) if name == "__super__") {
                    if self.check(&TokenKind::LParen) {
                        self.advance();
                        let mut args = Vec::new();
                        if !self.check(&TokenKind::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if !self.match_kind(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                        expr = Expr::SuperCall {
                            method: field,
                            args,
                            span: Span::merge(span, self.previous().span),
                        };
                    } else {
                        return Err(ParseError::Unexpected {
                            found: "member access on super".into(),
                            expected: "super.method() call".into(),
                            line: self.previous().span.line,
                            col: self.previous().span.col,
                        });
                    }
                } else {
                    expr = Expr::Member {
                        object: Box::new(expr),
                        field,
                        span,
                    };
                }
            } else if self.match_kind(&TokenKind::LBracket) {
                let index = self.parse_term()?;
                self.expect(TokenKind::RBracket)?;
                let span = Span::merge(expr.span(), self.previous().span);
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_span();
        let kind = self.advance().kind.clone();
        match kind {
            TokenKind::Int(v) => Ok(Expr::Int(v, start)),
            TokenKind::Float(v) => Ok(Expr::Float(v, start)),
            TokenKind::String(s) => Ok(Expr::String(s, start)),
            TokenKind::True => Ok(Expr::Bool(true, start)),
            TokenKind::False => Ok(Expr::Bool(false, start)),
            TokenKind::Nil => Ok(Expr::Nil(start)),
            TokenKind::Super => Ok(Expr::Ident("__super__".to_string(), start)),
            TokenKind::SelfKw => Ok(Expr::Ident("self".to_string(), start)),
            TokenKind::Ident(name) => {
                if self.check(&TokenKind::LBrace) && self.is_struct_init_ahead() {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) {
                        let fname = self.expect_ident()?;
                        self.expect(TokenKind::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RBrace)?;
                    if self.class_names.contains(&name) {
                        Ok(Expr::ClassInit {
                            name,
                            fields,
                            span: Span::merge(start, self.previous().span),
                        })
                    } else {
                        Ok(Expr::StructInit {
                            name,
                            fields,
                            span: Span::merge(start, self.previous().span),
                        })
                    }
                } else {
                    Ok(Expr::Ident(name, start))
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::LBracket => {
                let mut elements = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        elements.push(self.parse_expr()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Ok(Expr::Array {
                    elements,
                    span: Span::merge(start, self.previous().span),
                })
            }
            TokenKind::LBrace => {
                let mut fields = Vec::new();
                if !self.check(&TokenKind::RBrace) {
                    loop {
                        let fname = self.expect_ident()?;
                        self.expect(TokenKind::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(Expr::Object {
                    fields,
                    span: Span::merge(start, self.previous().span),
                })
            }
            other => Err(ParseError::Unexpected {
                found: format!("{other:?}"),
                expected: "expression".into(),
                line: self.previous().span.line,
                col: self.previous().span.col,
            }),
        }
    }

    fn is_struct_init_ahead(&self) -> bool {
        if !matches!(self.peek().kind, TokenKind::LBrace) {
            return false;
        }
        let field = self.pos + 1;
        let colon = self.pos + 2;
        if colon >= self.tokens.len() {
            return false;
        }
        matches!(self.tokens[field].kind, TokenKind::Ident(_))
            && matches!(self.tokens[colon].kind, TokenKind::Colon)
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.advance().kind.clone() {
            TokenKind::Ident(name) => Ok(name),
            TokenKind::SelfKw => Ok("self".to_string()),
            other => Err(ParseError::Unexpected {
                found: format!("{other:?}"),
                expected: "identifier".into(),
                line: self.previous().span.line,
                col: self.previous().span.col,
            }),
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        if self.check(&kind) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("{kind:?}")))
        }
    }

    fn match_kind(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            false
        } else {
            std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
        }
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.pos += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.pos.saturating_sub(1)]
    }

    fn current_span(&self) -> Span {
        self.peek().span
    }

    fn error(&self, expected: &str) -> ParseError {
        if self.is_at_end() {
            ParseError::Eof
        } else {
            ParseError::Unexpected {
                found: format!("{:?}", self.peek().kind),
                expected: expected.into(),
                line: self.peek().span.line,
                col: self.peek().span.col,
            }
        }
    }
}

pub fn parse(source: &str) -> Result<Program, ParseError> {
    let tokens = niao_lexer::lex(source).map_err(ParseError::from)?;
    Parser::new(tokens).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hello() {
        let src = r#"
fn greet(name: string) -> string {
    return "Hello, " + name
}
fn main() {
    print(greet("Niao"))
}
"#;
        let program = parse(src).unwrap();
        assert_eq!(program.items.len(), 2);
    }

    #[test]
    fn parses_fibonacci() {
        let src = include_str!("../../../examples/fibonacci.niao");
        let program = parse(src).unwrap();
        assert!(!program.items.is_empty());
    }

    #[test]
    fn parses_loops_example() {
        let src = include_str!("../../../examples/loops.niao");
        parse(src).unwrap();
    }

    #[test]
    fn parses_factorial_example() {
        let src = include_str!("../../../examples/factorial.niao");
        parse(src).unwrap();
    }

    #[test]
    fn parses_structs_example() {
        let src = include_str!("../../../examples/structs.niao");
        parse(src).unwrap();
    }

    #[test]
    fn parses_assignment_in_block() {
        let src = r#"fn f() { let x = 0; x = x + 1 }"#;
        parse(src).unwrap();
    }

    #[test]
    fn parses_index_assignment() {
        let src = r#"fn f(arr: array) { arr[0] = arr[1] }"#;
        parse(src).unwrap();
    }

    #[test]
    fn parses_index_not_comparison() {
        let src = r#"fn f(arr: array, j: int, key: int) { if arr[j] <= key { } }"#;
        let program = parse(src).unwrap();
        let cond = if let TopLevel::Fn(f) = &program.items[0] {
            if let Stmt::If { cond, .. } = &f.body.stmts[0] {
                cond
            } else {
                panic!("expected if")
            }
        } else {
            panic!("expected fn")
        };
        match cond {
            Expr::Binary { left, op: BinOp::Le, right, .. } => {
                assert!(matches!(&**left, Expr::Index { .. }));
                assert!(matches!(&**right, Expr::Ident(name, _) if name == "key"));
            }
            other => panic!("expected <= comparison, got {other:?}"),
        }
    }

    #[test]
    fn hyper_insertion_if_uses_int_index_not_bool() {
        let src = include_str!("../../../examples/super_booster_sort.niao");
        let program = parse(src).unwrap();
        let hyper = program
            .items
            .iter()
            .find_map(|item| {
                if let TopLevel::Fn(f) = item {
                    if f.name == "hyper_insertion" {
                        return Some(f);
                    }
                }
                None
            })
            .expect("hyper_insertion");
        let inner_if = {
            let outer_while = hyper
                .body
                .stmts
                .iter()
                .find_map(|s| {
                    if let Stmt::While { body, .. } = s {
                        Some(body)
                    } else {
                        None
                    }
                })
                .expect("outer while");
            let inner_while = outer_while
                .stmts
                .iter()
                .find_map(|s| {
                    if let Stmt::While { body, .. } = s {
                        Some(body)
                    } else {
                        None
                    }
                })
                .expect("inner while");
            inner_while
                .stmts
                .iter()
                .find_map(|s| {
                    if let Stmt::If { cond, .. } = s {
                        Some(cond)
                    } else {
                        None
                    }
                })
                .expect("inner if")
        };
        match inner_if {
            Expr::Binary { left, op: BinOp::Le, .. } => {
                match &**left {
                    Expr::Index { index, .. } => match &**index {
                        Expr::Ident(name, _) => assert_eq!(name, "j"),
                        other => panic!("index should be j, got {other:?}"),
                    },
                    other => panic!("left should be index, got {other:?}"),
                }
            }
            other => panic!("expected <=, got {other:?}"),
        }
    }

    #[test]
    fn parses_oop_basics() {
        let src = include_str!("../../../examples/oop_basics.niao");
        parse(src).unwrap();
    }
}
