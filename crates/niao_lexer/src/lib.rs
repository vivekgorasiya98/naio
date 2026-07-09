use niao_ast::Span;
use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub lexeme: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    True,
    False,
    Nil,

    // Identifiers & keywords
    Ident(String),
    Fn,
    Let,
    If,
    Else,
    While,
    For,
    In,
    Return,
    Import,
    As,
    Struct,
    Class,
    Trait,
    Extends,
    Implements,
    SelfKw,
    Super,
    Static,
    Public,
    Private,
    Try,
    Catch,
    Throw,
    Break,
    Continue,
    Server,

    // Types
    TypeInt,
    TypeFloat,
    TypeString,
    TypeBool,
    TypeVoid,
    TypeArray,

    // HTTP methods
    Get,
    Post,
    Put,
    Delete,
    Patch,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    FloorDiv,
    Percent,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Not,
    Assign,
    AddAssign,
    SubAssign,
    Arrow,

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Semicolon,

    Eof,
}

pub use niao_errors::LexError;

pub struct Lexer<'a> {
    source: &'a str,
    chars: Peekable<Chars<'a>>,
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars().peekable(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments();

        let start = self.pos;
        let line = self.line;
        let col = self.col;

        let Some(ch) = self.advance() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span {
                    start,
                    end: start,
                    line,
                    col,
                },
                lexeme: String::new(),
            });
        };

        let kind = match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
            '.' => TokenKind::Dot,
            ';' => TokenKind::Semicolon,
            '+' => {
                if self.match_char('=') {
                    TokenKind::AddAssign
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.match_char('>') {
                    TokenKind::Arrow
                } else if self.match_char('=') {
                    TokenKind::SubAssign
                } else {
                    TokenKind::Minus
                }
            }
            '*' => TokenKind::Star,
            '/' => {
                if self.match_char('/') {
                    self.skip_spaces_on_line();
                    let next = self.peek();
                    let is_floor_div = matches!(
                        next,
                        Some('0'..='9') | Some('(') | Some('-') | Some('+')
                    );
                    if is_floor_div {
                        TokenKind::FloorDiv
                    } else {
                        while matches!(self.peek(), Some(c) if c != '\n') {
                            self.advance();
                        }
                        return self.next_token();
                    }
                } else {
                    TokenKind::Slash
                }
            }
            '%' => TokenKind::Percent,
            '!' => {
                if self.match_char('=') {
                    TokenKind::Ne
                } else {
                    TokenKind::Not
                }
            }
            '=' => {
                if self.match_char('=') {
                    TokenKind::Eq
                } else {
                    TokenKind::Assign
                }
            }
            '<' => {
                if self.match_char('=') {
                    TokenKind::Le
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.match_char('=') {
                    TokenKind::Ge
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.match_char('&') {
                    TokenKind::And
                } else {
                    return Err(LexError::UnexpectedChar { ch, line, col });
                }
            }
            '|' => {
                if self.match_char('|') {
                    TokenKind::Or
                } else {
                    return Err(LexError::UnexpectedChar { ch, line, col });
                }
            }
            '"' => self.read_string(line, col)?,
            c if c.is_ascii_digit() => self.read_number(c, line, col)?,
            c if c.is_ascii_alphabetic() || c == '_' => self.read_ident(c, line, col)?,
            _ => return Err(LexError::UnexpectedChar { ch, line, col }),
        };

        let lexeme = self.source[start..self.pos].to_string();
        Ok(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
                line,
                col,
            },
            lexeme,
        })
    }

    fn read_string(&mut self, line: usize, col: usize) -> Result<TokenKind, LexError> {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch == '"' {
                self.advance();
                return Ok(TokenKind::String(s));
            }
            if ch == '\\' {
                self.advance();
                let escaped = self.advance().ok_or(LexError::UnterminatedString { line, col })?;
                match escaped {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    '\\' => s.push('\\'),
                    '"' => s.push('"'),
                    other => s.push(other),
                }
            } else if ch == '\n' {
                return Err(LexError::UnterminatedString { line, col });
            } else {
                s.push(self.advance().unwrap());
            }
        }
        Err(LexError::UnterminatedString { line, col })
    }

    fn read_number(&mut self, first: char, line: usize, col: usize) -> Result<TokenKind, LexError> {
        let start = self.pos - 1;

        if first == '0' && matches!(self.peek(), Some('x' | 'X')) {
            self.advance();
            if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                return Err(LexError::UnexpectedChar {
                    ch: 'x',
                    line,
                    col,
                });
            }
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.advance();
            }
            let text = &self.source[start..self.pos];
            let val = i64::from_str_radix(&text[2..], 16).map_err(|_| LexError::UnexpectedChar {
                ch: first,
                line,
                col,
            })?;
            return Ok(TokenKind::Int(val));
        }

        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.advance();
        }
        if self.peek() == Some('.') {
            self.advance();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.advance();
            }
            let text = &self.source[start..self.pos];
            let val: f64 = text.parse().map_err(|_| LexError::UnexpectedChar {
                ch: first,
                line,
                col,
            })?;
            return Ok(TokenKind::Float(val));
        }
        let text = &self.source[start..self.pos];
        let val: i64 = text.parse().map_err(|_| LexError::UnexpectedChar {
            ch: first,
            line,
            col,
        })?;
        Ok(TokenKind::Int(val))
    }

    fn read_ident(&mut self, _first: char, _line: usize, _col: usize) -> Result<TokenKind, LexError> {
        let start = self.pos - 1;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.advance();
        }
        let text = &self.source[start..self.pos];
        let kind = match text {
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" => TokenKind::Return,
            "import" => TokenKind::Import,
            "as" => TokenKind::As,
            "struct" => TokenKind::Struct,
            "class" => TokenKind::Class,
            "trait" => TokenKind::Trait,
            "extends" => TokenKind::Extends,
            "implements" => TokenKind::Implements,
            "self" => TokenKind::SelfKw,
            "super" => TokenKind::Super,
            "static" => TokenKind::Static,
            "public" => TokenKind::Public,
            "private" => TokenKind::Private,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "throw" => TokenKind::Throw,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "server" => TokenKind::Server,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "nil" => TokenKind::Nil,
            "int" => TokenKind::TypeInt,
            "float" => TokenKind::TypeFloat,
            "string" => TokenKind::TypeString,
            "bool" => TokenKind::TypeBool,
            "void" => TokenKind::TypeVoid,
            "array" => TokenKind::TypeArray,
            "GET" => TokenKind::Get,
            "POST" => TokenKind::Post,
            "PUT" => TokenKind::Put,
            "DELETE" => TokenKind::Delete,
            "PATCH" => TokenKind::Patch,
            other => TokenKind::Ident(other.to_string()),
        };
        Ok(kind)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }
            if self.peek() == Some('#') {
                self.advance();
                while matches!(self.peek(), Some(c) if c != '\n') {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn skip_spaces_on_line(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r')) {
            self.advance();
        }
    }
}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(source).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_hello() {
        let tokens = lex(r#"fn main() { print("hi") }"#).unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Fn)));
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "main")));
    }

    #[test]
    fn lexes_hex_integers() {
        let tokens = lex(r#"let b = [0x48, 0x69];"#).unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Int(72))));
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Int(105))));
    }

    #[test]
    fn lexes_numbers_and_strings() {
        let tokens = lex(r#"let x = 42; let y = 3.14; let s = "hello";"#).unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Int(42))));
        assert!(tokens
            .iter()
            .any(|t| matches!(&t.kind, TokenKind::Float(f) if (*f - 3.14).abs() < 0.001)));
        assert!(tokens
            .iter()
            .any(|t| matches!(&t.kind, TokenKind::String(s) if s == "hello")));
    }

    #[test]
    fn lexes_http_methods() {
        let tokens = lex(r#"GET "/users" { }"#).unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Get)));
    }
}
