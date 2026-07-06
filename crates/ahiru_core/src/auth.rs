use crate::context::RequestContext;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    None,
    Jwt,
    Session,
    ApiKey,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub scope: String,
    pub jwt_secret: String,
    pub session_secret: String,
    pub api_keys: HashSet<String>,
    pub rbac_enabled: bool,
}

impl AuthConfig {
    pub fn from_file(f: &crate::config::AuthConfigFile) -> Self {
        let mode = match f.mode.to_lowercase().as_str() {
            "jwt" => AuthMode::Jwt,
            "session" => AuthMode::Session,
            "api_key" | "apikey" | "api-key" => AuthMode::ApiKey,
            _ => AuthMode::None,
        };
        Self {
            mode,
            scope: f.scope.clone(),
            jwt_secret: f.jwt_secret.clone().unwrap_or_else(|| "dev-secret-change-me".into()),
            session_secret: f
                .session_secret
                .clone()
                .unwrap_or_else(|| "session-dev-secret".into()),
            api_keys: f.api_keys.iter().cloned().collect(),
            rbac_enabled: f.rbac_enabled,
        }
    }

    pub fn authenticate(&self, ctx: &RequestContext) -> Result<Option<UserContext>, String> {
        match self.mode {
            AuthMode::None => Ok(None),
            AuthMode::Jwt => self.authenticate_jwt(ctx),
            AuthMode::Session => self.authenticate_session(ctx),
            AuthMode::ApiKey => self.authenticate_api_key(ctx),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub exp: usize,
}

#[derive(Debug, Clone)]
pub struct UserContext {
    pub id: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

impl AuthConfig {
    fn authenticate_jwt(&self, ctx: &RequestContext) -> Result<Option<UserContext>, String> {
        let auth = ctx.header("authorization").ok_or("missing authorization header")?;
        let token = auth
            .strip_prefix("Bearer ")
            .or_else(|| auth.strip_prefix("bearer "))
            .ok_or("expected Bearer token")?;
        let data = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|e| e.to_string())?;
        Ok(Some(UserContext {
            id: data.claims.sub,
            roles: data.claims.roles,
            permissions: data.claims.permissions,
        }))
    }

    fn authenticate_session(&self, ctx: &RequestContext) -> Result<Option<UserContext>, String> {
        let cookie = ctx
            .header("cookie")
            .ok_or("missing session cookie")?;
        let session = cookie
            .split(';')
            .find_map(|p| {
                let p = p.trim();
                p.strip_prefix("ahiru_session=")
            })
            .ok_or("ahiru_session cookie not found")?;
        let user = verify_session_token(session, &self.session_secret)?;
        Ok(Some(user))
    }

    fn authenticate_api_key(&self, ctx: &RequestContext) -> Result<Option<UserContext>, String> {
        let key = ctx
            .header("x-api-key")
            .ok_or("missing X-API-Key header")?;
        if !self.api_keys.contains(key) {
            return Err("invalid API key".into());
        }
        Ok(Some(UserContext {
            id: format!("apikey:{}", &key[..key.len().min(8)]),
            roles: vec!["api".into()],
            permissions: vec!["*".into()],
        }))
    }

    pub fn issue_jwt(&self, user_id: &str, roles: &[String], permissions: &[String], exp_secs: usize) -> Result<String, String> {
        let exp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs() as usize
            + exp_secs;
        let claims = JwtClaims {
            sub: user_id.into(),
            roles: roles.to_vec(),
            permissions: permissions.to_vec(),
            exp,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| e.to_string())
    }

    pub fn issue_session_cookie(&self, user_id: &str, roles: &[String], permissions: &[String]) -> Result<String, String> {
        let token = sign_session_token(user_id, roles, permissions, &self.session_secret)?;
        Ok(format!("ahiru_session={token}; HttpOnly; Path=/; SameSite=Lax"))
    }
}

fn sign_session_token(
    user_id: &str,
    roles: &[String],
    permissions: &[String],
    secret: &str,
) -> Result<String, String> {
    let payload = format!(
        "{}|{}|{}",
        user_id,
        roles.join(","),
        permissions.join(",")
    );
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(payload.as_bytes());
    let sig = format!("{:x}", hasher.finalize());
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        format!("{payload}.{sig}"),
    ))
}

fn verify_session_token(token: &str, secret: &str) -> Result<UserContext, String> {
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, token)
        .map_err(|e| e.to_string())?;
    let s = String::from_utf8(decoded).map_err(|e| e.to_string())?;
    let (payload, sig) = s.rsplit_once('.').ok_or("invalid session token")?;
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(payload.as_bytes());
    let expected = format!("{:x}", hasher.finalize());
    if sig != expected {
        return Err("session signature mismatch".into());
    }
    let mut parts = payload.split('|');
    let id = parts.next().ok_or("invalid session payload")?.into();
    let roles: Vec<String> = parts
        .next()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let permissions: Vec<String> = parts
        .next()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    Ok(UserContext {
        id,
        roles,
        permissions,
    })
}
