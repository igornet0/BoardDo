//! Runtime expression / template evaluation for SmartDo.
//!
//! Supports:
//! - Paths: `trigger.amount`, `nodes.get_user.output.name`, `variables.x`, `amount`
//! - Comparisons: `== != > >= < <=`
//! - Arithmetic: `+ - * /`
//! - String equality with quoted literals
//! - Embedded templates: `Hello {{trigger.name}}`

use serde_json::{Number, Value};

use super::context::ExecutionContext;

/// Resolve a template string. Pure path → typed Value; mixed text → String;
/// expressions with operators → evaluated Value.
pub fn resolve(ctx: &ExecutionContext, template: &str) -> Result<Value, String> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Ok(Value::String(String::new()));
    }

    // Whole-mustache: {{ ... }}
    if let Some(inner) = strip_mustache(trimmed) {
        return eval_inner(ctx, inner);
    }

    // Embedded mustaches in a string
    if trimmed.contains("{{") {
        return Ok(Value::String(interpolate_string(ctx, trimmed)?));
    }

    Ok(Value::String(trimmed.to_string()))
}

/// Resolve a Value that may be a template string or a literal.
pub fn resolve_value(ctx: &ExecutionContext, raw: Value) -> Result<Value, String> {
    match raw {
        Value::String(s) => resolve(ctx, &s),
        other => Ok(other),
    }
}

fn strip_mustache(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.starts_with("{{") && s.ends_with("}}") && s.matches("{{").count() == 1 {
        Some(s[2..s.len() - 2].trim())
    } else {
        None
    }
}

fn interpolate_string(ctx: &ExecutionContext, template: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            result.push_str(&rest[start..]);
            return Ok(result);
        };
        let inner = after[..end].trim();
        let value = eval_inner(ctx, inner)?;
        result.push_str(&value_to_display(&value));
        rest = &after[end + 2..];
    }
    result.push_str(rest);
    Ok(result)
}

fn value_to_display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string().trim_matches('"').to_string(),
    }
}

fn eval_inner(ctx: &ExecutionContext, expr: &str) -> Result<Value, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Ok(Value::Null);
    }
    // Fast path: pure identifier / dotted path (no operators)
    if is_path_only(expr) {
        return Ok(ctx.resolve_path(expr).unwrap_or(Value::Null));
    }
    eval_expression(ctx, expr)
}

fn is_path_only(expr: &str) -> bool {
    !expr.chars().any(|c| {
        matches!(
            c,
            '+' | '-' | '*' | '/' | '=' | '!' | '<' | '>' | '(' | ')' | '"' | '\'' | ' '
        )
    }) && !expr.is_empty()
        && expr
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

// ─── Expression parser (precedence climbing) ─────────────────────────────────

#[derive(Debug, Clone)]
enum Token {
    Number(f64),
    String(String),
    Bool(bool),
    Ident(String),
    Op(String),
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' => i += 1,
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '"' | '\'' => {
                let quote = chars[i];
                i += 1;
                let start = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("unterminated string literal".into());
                }
                let s: String = chars[start..i].iter().collect();
                tokens.push(Token::String(s));
                i += 1;
            }
            c if c.is_ascii_digit()
                || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) =>
            {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let n: f64 = s.parse().map_err(|_| format!("invalid number `{s}`"))?;
                tokens.push(Token::Number(n));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                match s.as_str() {
                    "true" => tokens.push(Token::Bool(true)),
                    "false" => tokens.push(Token::Bool(false)),
                    "null" => tokens.push(Token::Ident("null".into())), // resolved as path miss → null via eval
                    _ => tokens.push(Token::Ident(s)),
                }
            }
            '=' | '!' | '<' | '>' => {
                let ch = chars[i];
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::Op(format!("{ch}=")));
                    i += 2;
                } else if ch == '<' || ch == '>' {
                    tokens.push(Token::Op(ch.to_string()));
                    i += 1;
                } else if ch == '=' {
                    tokens.push(Token::Op("==".into()));
                    i += 1;
                } else {
                    return Err(format!("unexpected `{ch}`"));
                }
            }
            '+' | '-' | '*' | '/' => {
                tokens.push(Token::Op(chars[i].to_string()));
                i += 1;
            }
            '&' if i + 1 < chars.len() && chars[i + 1] == '&' => {
                tokens.push(Token::Op("&&".into()));
                i += 2;
            }
            '|' if i + 1 < chars.len() && chars[i + 1] == '|' => {
                tokens.push(Token::Op("||".into()));
                i += 2;
            }
            other => return Err(format!("unexpected character `{other}` in expression")),
        }
    }
    Ok(tokens)
}

fn precedence(op: &str) -> i32 {
    match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" | "<" | "<=" | ">" | ">=" => 3,
        "+" | "-" => 4,
        "*" | "/" => 5,
        _ => 0,
    }
}

fn eval_expression(ctx: &ExecutionContext, expr: &str) -> Result<Value, String> {
    let tokens = tokenize(expr)?;
    let mut pos = 0;
    let value = parse_expr(ctx, &tokens, &mut pos, 0)?;
    if pos < tokens.len() {
        return Err(format!("unexpected token at end of expression `{expr}`"));
    }
    Ok(value)
}

fn parse_expr(
    ctx: &ExecutionContext,
    tokens: &[Token],
    pos: &mut usize,
    min_prec: i32,
) -> Result<Value, String> {
    let mut left = parse_primary(ctx, tokens, pos)?;

    while *pos < tokens.len() {
        let Token::Op(op) = &tokens[*pos] else {
            break;
        };
        let prec = precedence(op);
        if prec < min_prec {
            break;
        }
        let op = op.clone();
        *pos += 1;
        let right = parse_expr(ctx, tokens, pos, prec + 1)?;
        left = apply_op(&op, &left, &right)?;
    }
    Ok(left)
}

fn parse_primary(
    ctx: &ExecutionContext,
    tokens: &[Token],
    pos: &mut usize,
) -> Result<Value, String> {
    if *pos >= tokens.len() {
        return Err("unexpected end of expression".into());
    }
    match &tokens[*pos] {
        Token::Number(n) => {
            *pos += 1;
            Ok(json_number(*n))
        }
        Token::String(s) => {
            *pos += 1;
            Ok(Value::String(s.clone()))
        }
        Token::Bool(b) => {
            *pos += 1;
            Ok(Value::Bool(*b))
        }
        Token::Ident(name) => {
            *pos += 1;
            if name == "null" {
                return Ok(Value::Null);
            }
            // Unary minus handled via Op before number; idents are paths
            Ok(ctx.resolve_path(name).unwrap_or(Value::Null))
        }
        Token::Op(op) if op == "-" => {
            *pos += 1;
            let v = parse_primary(ctx, tokens, pos)?;
            Ok(json_number(-as_f64(&v)?))
        }
        Token::LParen => {
            *pos += 1;
            let v = parse_expr(ctx, tokens, pos, 0)?;
            match tokens.get(*pos) {
                Some(Token::RParen) => {
                    *pos += 1;
                    Ok(v)
                }
                _ => Err("missing closing parenthesis".into()),
            }
        }
        other => Err(format!("unexpected token in expression: {other:?}")),
    }
}

fn apply_op(op: &str, left: &Value, right: &Value) -> Result<Value, String> {
    match op {
        "==" => Ok(Value::Bool(values_equal(left, right))),
        "!=" => Ok(Value::Bool(!values_equal(left, right))),
        ">" => Ok(Value::Bool(as_f64(left)? > as_f64(right)?)),
        ">=" => Ok(Value::Bool(as_f64(left)? >= as_f64(right)?)),
        "<" => Ok(Value::Bool(as_f64(left)? < as_f64(right)?)),
        "<=" => Ok(Value::Bool(as_f64(left)? <= as_f64(right)?)),
        "+" => {
            if let (Value::String(a), _) = (left, right) {
                return Ok(Value::String(format!("{}{}", a, value_to_display(right))));
            }
            if let (_, Value::String(b)) = (left, right) {
                return Ok(Value::String(format!("{}{}", value_to_display(left), b)));
            }
            Ok(json_number(as_f64(left)? + as_f64(right)?))
        }
        "-" => Ok(json_number(as_f64(left)? - as_f64(right)?)),
        "*" => Ok(json_number(as_f64(left)? * as_f64(right)?)),
        "/" => {
            let r = as_f64(right)?;
            if r == 0.0 {
                return Err("division by zero".into());
            }
            Ok(json_number(as_f64(left)? / r))
        }
        "&&" => Ok(Value::Bool(as_bool(left) && as_bool(right))),
        "||" => Ok(Value::Bool(as_bool(left) || as_bool(right))),
        _ => Err(format!("unknown operator `{op}`")),
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    // Coerce number-like strings
    if let (Ok(x), Ok(y)) = (as_f64(a), as_f64(b)) {
        return (x - y).abs() < f64::EPSILON;
    }
    false
}

fn as_f64(v: &Value) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "number out of range".to_string()),
        Value::String(s) => s
            .parse::<f64>()
            .map_err(|_| format!("cannot coerce `{s}` to number")),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        _ => Err(format!("cannot coerce {v} to number")),
    }
}

fn as_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn json_number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
        Value::Number(Number::from(n as i64))
    } else {
        Number::from_f64(n)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::engine::ExecutionContext;

    fn ctx() -> ExecutionContext {
        let mut c =
            ExecutionContext::new(Uuid::now_v7(), Uuid::now_v7(), 1, json!({"amount": 150}));
        c.set_variable("company_id", json!("acme"));
        c.set_node_output("get_user", json!({"name": "Igor", "status": "active"}));
        c
    }

    #[test]
    fn path_trigger() {
        let c = ctx();
        assert_eq!(resolve(&c, "{{trigger.amount}}").unwrap(), json!(150));
    }

    #[test]
    fn path_nodes_output() {
        let c = ctx();
        assert_eq!(
            resolve(&c, "{{nodes.get_user.output.name}}").unwrap(),
            json!("Igor")
        );
        assert_eq!(
            resolve(&c, "{{nodes.get_user.name}}").unwrap(),
            json!("Igor")
        );
    }

    #[test]
    fn path_variables() {
        let c = ctx();
        assert_eq!(
            resolve(&c, "{{variables.company_id}}").unwrap(),
            json!("acme")
        );
    }

    #[test]
    fn compare_and_arith() {
        let c = ctx();
        assert_eq!(
            resolve(&c, "{{trigger.amount > 100}}").unwrap(),
            json!(true)
        );
        assert_eq!(
            resolve(&c, "{{nodes.get_user.status == \"active\"}}").unwrap(),
            json!(true)
        );
        assert_eq!(resolve(&c, "{{trigger.amount * 0.2}}").unwrap(), json!(30));
    }

    #[test]
    fn interpolate() {
        let c = ctx();
        assert_eq!(
            resolve(&c, "Hello {{nodes.get_user.name}}").unwrap(),
            json!("Hello Igor")
        );
    }
}
