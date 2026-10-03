//! Inline calculator: a small recursive-descent expression evaluator.
//!
//! Grammar (lowest to highest precedence):
//!
//! ```text
//! expr    = term   (("+" | "-") term)*
//! term    = unary  (("*" | "/" | "%" | "×" | "÷") unary)*
//! unary   = ("+" | "-") unary | power
//! power   = postfix ("^" unary)?          (right-associative; "**" is "^")
//! postfix = primary "!"*
//! primary = number | constant | function "(" expr ("," expr)* ")" | "(" expr ")"
//! ```
//!
//! Unary minus binds looser than `^`, so `-2^2 = -4`, while the exponent may
//! itself be signed (`2^-2 = 0.25`).
//!
//! The plugin also answers conversions: units offline (`10 km in mi`, see
//! [`crate::units`]) and, when `[calculator] currency = true`, fiat currencies
//! (`100 usd in eur`, see [`crate::currency`]).

use std::fmt;
use std::sync::Arc;

use sevak_core::model::score;
use sevak_core::{Action, Config, IconSource, Plugin, PluginResult, ResultItem};
use sevak_platform::{AppPaths, PlatformProvider};

use crate::actions::execute_action;
use crate::currency::{self, RateService};
use crate::units;

const MAX_NESTING: usize = 64;
const SIGNIFICANT_DIGITS: i32 = 12;

#[derive(Debug, Clone, PartialEq)]
pub enum CalcError {
    /// A character or token that cannot appear at this point.
    UnexpectedToken {
        position: usize,
        found: String,
    },
    UnexpectedEnd,
    UnknownFunction {
        position: usize,
        name: String,
    },
    UnknownConstant {
        position: usize,
        name: String,
    },
    WrongArity {
        name: String,
        expected: usize,
        found: usize,
    },
    DivisionByZero,
    /// Factorial needs an integer in `0..=170`.
    InvalidFactorial,
    /// The result is infinite or not a number (`sqrt(-1)`, `10^400`, ...).
    NotFinite,
    TooDeeplyNested,
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedToken { position, found } => {
                write!(f, "unexpected `{found}` at position {position}")
            }
            Self::UnexpectedEnd => write!(f, "unexpected end of expression"),
            Self::UnknownFunction { position, name } => {
                write!(f, "unknown function `{name}` at position {position}")
            }
            Self::UnknownConstant { position, name } => {
                write!(f, "unknown name `{name}` at position {position}")
            }
            Self::WrongArity {
                name,
                expected,
                found,
            } => write!(
                f,
                "`{name}` takes {expected} argument{}, got {found}",
                if *expected == 1 { "" } else { "s" }
            ),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::InvalidFactorial => {
                write!(f, "factorial needs an integer between 0 and 170")
            }
            Self::NotFinite => write!(f, "result is not a finite number"),
            Self::TooDeeplyNested => write!(f, "expression is nested too deeply"),
        }
    }
}

impl std::error::Error for CalcError {}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Bang,
    LParen,
    RParen,
    Comma,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    /// Character index in the (trimmed, `=`-stripped) source.
    pos: usize,
    /// Canonical text used to render the normalized expression.
    text: String,
    /// Original source text, for error messages.
    source: String,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn tokenize(src: &str) -> Result<Vec<Token>, CalcError> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let simple = |tok: Tok, text: &str, source: &str| Token {
            tok,
            pos: start,
            text: text.to_owned(),
            source: source.to_owned(),
        };

        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let mut text = String::new();
            let digits = |i: &mut usize, text: &mut String| {
                while *i < chars.len() && (chars[*i].is_ascii_digit() || chars[*i] == '_') {
                    if chars[*i] != '_' {
                        text.push(chars[*i]);
                    }
                    *i += 1;
                }
            };
            digits(&mut i, &mut text);
            if chars.get(i) == Some(&'.') {
                text.push('.');
                i += 1;
                digits(&mut i, &mut text);
            }
            if matches!(chars.get(i), Some('e' | 'E')) {
                let mut j = i + 1;
                if matches!(chars.get(j), Some('+' | '-')) {
                    j += 1;
                }
                if chars.get(j).is_some_and(char::is_ascii_digit) {
                    text.push('e');
                    if matches!(chars[i + 1], '+' | '-') {
                        text.push(chars[i + 1]);
                    }
                    i = j;
                    digits(&mut i, &mut text);
                }
            }
            let value: f64 = text.parse().map_err(|_| CalcError::UnexpectedToken {
                position: start,
                found: chars[start..i].iter().collect(),
            })?;
            tokens.push(Token {
                tok: Tok::Num(value),
                pos: start,
                text,
                source: chars[start..i].iter().collect(),
            });
            continue;
        }

        if is_ident_start(c) {
            while i < chars.len() && is_ident_continue(chars[i]) {
                i += 1;
            }
            let source: String = chars[start..i].iter().collect();
            let name = source.to_lowercase();
            tokens.push(Token {
                tok: Tok::Ident(name.clone()),
                pos: start,
                text: name,
                source,
            });
            continue;
        }

        i += 1;
        let token = match c {
            '+' => simple(Tok::Plus, "+", "+"),
            '-' | '\u{2212}' => simple(Tok::Minus, "-", &c.to_string()),
            '*' if chars.get(i) == Some(&'*') => {
                i += 1;
                simple(Tok::Caret, "^", "**")
            }
            '*' | '×' => simple(Tok::Star, "*", &c.to_string()),
            '/' | '÷' => simple(Tok::Slash, "/", &c.to_string()),
            '%' => simple(Tok::Percent, "%", "%"),
            '^' => simple(Tok::Caret, "^", "^"),
            '!' => simple(Tok::Bang, "!", "!"),
            '(' => simple(Tok::LParen, "(", "("),
            ')' => simple(Tok::RParen, ")", ")"),
            ',' => simple(Tok::Comma, ",", ","),
            other => {
                return Err(CalcError::UnexpectedToken {
                    position: start,
                    found: other.to_string(),
                })
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

fn constant(name: &str) -> Option<f64> {
    match name {
        "pi" | "π" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        "tau" => Some(std::f64::consts::TAU),
        _ => None,
    }
}

/// Number of arguments `name` takes, if it is a known function.
fn arity(name: &str) -> Option<usize> {
    match name {
        "sqrt" | "cbrt" | "abs" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh"
        | "cosh" | "tanh" | "ln" | "log" | "log2" | "exp" | "floor" | "ceil" | "round" | "sign" => {
            Some(1)
        }
        "min" | "max" | "pow" | "hypot" | "atan2" => Some(2),
        _ => None,
    }
}

fn apply(name: &str, args: &[f64]) -> f64 {
    let x = args[0];
    match name {
        "sqrt" => x.sqrt(),
        "cbrt" => x.cbrt(),
        "abs" => x.abs(),
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "atan" => x.atan(),
        "sinh" => x.sinh(),
        "cosh" => x.cosh(),
        "tanh" => x.tanh(),
        "ln" => x.ln(),
        "log" => x.log10(),
        "log2" => x.log2(),
        "exp" => x.exp(),
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "round" => x.round(),
        "sign" => {
            if x == 0.0 {
                0.0
            } else {
                x.signum()
            }
        }
        "min" => x.min(args[1]),
        "max" => x.max(args[1]),
        "pow" => x.powf(args[1]),
        "hypot" => x.hypot(args[1]),
        "atan2" => x.atan2(args[1]),
        _ => unreachable!("arity() vetted the function name"),
    }
}

fn factorial(x: f64) -> Result<f64, CalcError> {
    if !(x.fract() == 0.0 && (0.0..=170.0).contains(&x)) {
        return Err(CalcError::InvalidFactorial);
    }
    let mut result = 1.0;
    let mut n = 2.0;
    while n <= x {
        result *= n;
        n += 1.0;
    }
    Ok(result)
}

/// A value together with the magnitude that bounds its rounding noise.
///
/// Floating point error is relative to the numbers that went into a result, not
/// to the result itself: `pi - 3.14159265358979` is mostly noise because both
/// operands are about 3, even though the answer is about 1e-15. `scale` tracks
/// that "size of the operands" so [`snap_noise`] can tell cancellation noise
/// (`sin(pi)`) from a result that is genuinely tiny (`2e-20 * 3`).
#[derive(Debug, Clone, Copy)]
struct Num {
    value: f64,
    scale: f64,
}

impl Num {
    /// An exact input (literal or constant): its own size is the scale.
    fn exact(value: f64) -> Self {
        Self {
            value,
            scale: value.abs(),
        }
    }

    /// A result whose operands had the given `scale`; never below its own size.
    fn result(value: f64, scale: f64) -> Self {
        let own = value.abs();
        Self {
            value,
            scale: if scale.is_finite() {
                scale.max(own)
            } else {
                own
            },
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.index)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.index);
        self.index += 1;
        token
    }

    fn unexpected(token: &Token) -> CalcError {
        CalcError::UnexpectedToken {
            position: token.pos,
            found: token.source.clone(),
        }
    }

    fn enter(&mut self) -> Result<(), CalcError> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            Err(CalcError::TooDeeplyNested)
        } else {
            Ok(())
        }
    }

    fn expr(&mut self) -> Result<Num, CalcError> {
        self.enter()?;
        let mut acc = self.term()?;
        while let Some(token) = self.peek() {
            let subtract = match token.tok {
                Tok::Plus => false,
                Tok::Minus => true,
                _ => break,
            };
            self.index += 1;
            let rhs = self.term()?;
            let value = if subtract {
                acc.value - rhs.value
            } else {
                acc.value + rhs.value
            };
            // Addition is where cancellation happens: noise is relative to the
            // larger operand.
            acc = Num::result(value, acc.scale.max(rhs.scale));
        }
        self.depth -= 1;
        Ok(acc)
    }

    fn term(&mut self) -> Result<Num, CalcError> {
        let mut acc = self.unary()?;
        while let Some(token) = self.peek() {
            match token.tok {
                Tok::Star => {
                    self.index += 1;
                    let rhs = self.unary()?;
                    let scale = (acc.scale * rhs.value.abs()).max(rhs.scale * acc.value.abs());
                    acc = Num::result(acc.value * rhs.value, scale);
                }
                Tok::Slash => {
                    self.index += 1;
                    let rhs = self.unary()?;
                    if rhs.value == 0.0 {
                        return Err(CalcError::DivisionByZero);
                    }
                    acc = Num::result(acc.value / rhs.value, acc.scale / rhs.value.abs());
                }
                Tok::Percent => {
                    self.index += 1;
                    let rhs = self.unary()?;
                    if rhs.value == 0.0 {
                        return Err(CalcError::DivisionByZero);
                    }
                    acc = Num::result(acc.value % rhs.value, 0.0);
                }
                _ => break,
            }
        }
        Ok(acc)
    }

    fn unary(&mut self) -> Result<Num, CalcError> {
        match self.peek().map(|t| &t.tok) {
            Some(Tok::Plus) => {
                self.index += 1;
                self.enter()?;
                let value = self.unary();
                self.depth -= 1;
                value
            }
            Some(Tok::Minus) => {
                self.index += 1;
                self.enter()?;
                let value = self.unary();
                self.depth -= 1;
                let n = value?;
                Ok(Num {
                    value: -n.value,
                    scale: n.scale,
                })
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Num, CalcError> {
        let base = self.postfix()?;
        if matches!(self.peek().map(|t| &t.tok), Some(Tok::Caret)) {
            self.index += 1;
            self.enter()?;
            let exponent = self.unary();
            self.depth -= 1;
            return Ok(Num::result(base.value.powf(exponent?.value), 0.0));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Num, CalcError> {
        let mut n = self.primary()?;
        while matches!(self.peek().map(|t| &t.tok), Some(Tok::Bang)) {
            self.index += 1;
            n = Num::result(factorial(n.value)?, 0.0);
        }
        Ok(n)
    }

    fn primary(&mut self) -> Result<Num, CalcError> {
        let token = self.next().ok_or(CalcError::UnexpectedEnd)?;
        match &token.tok {
            Tok::Num(value) => Ok(Num::exact(*value)),
            Tok::LParen => {
                let n = self.expr()?;
                self.expect_close()?;
                Ok(n)
            }
            Tok::Ident(name) => {
                let is_call = matches!(self.peek().map(|t| &t.tok), Some(Tok::LParen));
                if is_call {
                    self.index += 1;
                    self.call(name, token.pos)
                } else if let Some(value) = constant(name) {
                    Ok(Num::exact(value))
                } else {
                    Err(CalcError::UnknownConstant {
                        position: token.pos,
                        name: token.source.clone(),
                    })
                }
            }
            _ => Err(Self::unexpected(token)),
        }
    }

    fn call(&mut self, name: &str, position: usize) -> Result<Num, CalcError> {
        let expected = arity(name).ok_or_else(|| CalcError::UnknownFunction {
            position,
            name: name.to_owned(),
        })?;
        let mut args: Vec<Num> = Vec::new();
        if matches!(self.peek().map(|t| &t.tok), Some(Tok::RParen)) {
            self.index += 1;
        } else {
            loop {
                args.push(self.expr()?);
                match self.next() {
                    Some(Token {
                        tok: Tok::Comma, ..
                    }) => continue,
                    Some(Token {
                        tok: Tok::RParen, ..
                    }) => break,
                    Some(other) => return Err(Self::unexpected(other)),
                    None => return Err(CalcError::UnexpectedEnd),
                }
            }
        }
        if args.len() != expected {
            return Err(CalcError::WrongArity {
                name: name.to_owned(),
                expected,
                found: args.len(),
            });
        }
        let values: Vec<f64> = args.iter().map(|a| a.value).collect();
        // `sin(pi)` is noise next to `pi`: the arguments' scale carries over.
        let scale = args.iter().fold(0.0_f64, |m, a| m.max(a.scale));
        Ok(Num::result(apply(name, &values), scale))
    }

    fn expect_close(&mut self) -> Result<(), CalcError> {
        match self.next() {
            Some(Token {
                tok: Tok::RParen, ..
            }) => Ok(()),
            Some(other) => Err(Self::unexpected(other)),
            None => Err(CalcError::UnexpectedEnd),
        }
    }
}

fn strip_prefix(input: &str) -> &str {
    let input = input.trim();
    input.strip_prefix('=').unwrap_or(input).trim_start()
}

/// Relative size below which a result is considered cancellation noise.
const NOISE_THRESHOLD: f64 = 1e-12;

/// Snaps `value` to zero when it is negligible next to the magnitude (`scale`)
/// of the operands that produced it. `sin(pi)` is `1.2e-16` only because `pi` is
/// rounded, and `pi` (3.14) dwarfs it; but `1e-15` on its own, or `2e-20 * 3`,
/// *is* the biggest thing in its expression and stays intact.
fn snap_noise(value: f64, scale: f64) -> f64 {
    if value.abs() < NOISE_THRESHOLD * scale {
        0.0
    } else {
        value
    }
}

fn evaluate_tokens(tokens: &[Token]) -> Result<f64, CalcError> {
    let mut parser = Parser {
        tokens,
        index: 0,
        depth: 0,
    };
    let result = parser.expr()?;
    if let Some(extra) = parser.peek() {
        return Err(Parser::unexpected(extra));
    }
    if result.value.is_finite() {
        Ok(snap_noise(result.value, result.scale))
    } else {
        Err(CalcError::NotFinite)
    }
}

/// Evaluates `expression` (an optional leading `=` is ignored).
pub fn evaluate(expression: &str) -> Result<f64, CalcError> {
    let tokens = tokenize(strip_prefix(expression))?;
    evaluate_tokens(&tokens)
}

fn ends_value(tok: &Tok) -> bool {
    matches!(tok, Tok::Num(_) | Tok::Ident(_) | Tok::RParen | Tok::Bang)
}

/// Whether the input looks like a calculation rather than a stray number,
/// constant or app name: it needs a value (digit or constant) and a binary
/// operator, function call or factorial.
fn is_math_like(tokens: &[Token]) -> bool {
    let has_value = tokens.iter().any(|t| match &t.tok {
        Tok::Num(_) => true,
        Tok::Ident(name) => constant(name).is_some(),
        _ => false,
    });
    if !has_value {
        return false;
    }
    tokens.iter().enumerate().any(|(i, t)| match &t.tok {
        Tok::Bang => true,
        Tok::Ident(_) => matches!(tokens.get(i + 1).map(|n| &n.tok), Some(Tok::LParen)),
        Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash | Tok::Percent | Tok::Caret => {
            i > 0 && ends_value(&tokens[i - 1].tok)
        }
        _ => false,
    })
}

/// Canonical rendering: aliases resolved, binary operators spaced.
fn normalize(tokens: &[Token]) -> String {
    let mut out = String::new();
    for (i, token) in tokens.iter().enumerate() {
        match &token.tok {
            Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash | Tok::Percent | Tok::Caret
                if i > 0 && ends_value(&tokens[i - 1].tok) =>
            {
                out.push(' ');
                out.push_str(&token.text);
                out.push(' ');
            }
            Tok::Comma => out.push_str(", "),
            _ => out.push_str(&token.text),
        }
    }
    out
}

/// Evaluates `input` if it is math-like; `None` for anything else, including
/// parse errors and non-finite results.
pub fn answer(input: &str) -> Option<(String, f64)> {
    let tokens = tokenize(strip_prefix(input)).ok()?;
    if !is_math_like(&tokens) {
        return None;
    }
    let value = evaluate_tokens(&tokens).ok()?;
    Some((normalize(&tokens), value))
}

/// Formats a result for display and copying.
///
/// Integers within ±1e15 print without decimals; other values use up to 12
/// significant digits with trailing zeros trimmed; magnitudes of at least 1e15
/// or below 1e-9 switch to scientific notation (`1.234e+20`).
pub fn format_number(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return "0".to_owned();
    }
    let magnitude = value.abs();
    if !(1e-9..1e15).contains(&magnitude) {
        let rendered = format!("{:.*e}", (SIGNIFICANT_DIGITS - 1) as usize, value);
        let (mantissa, exponent) = rendered.split_once('e').expect("`{:e}` has an exponent");
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exponent: i32 = exponent.parse().expect("`{:e}` exponent is an integer");
        return format!("{mantissa}e{exponent:+}");
    }
    if value.fract() == 0.0 {
        return format!("{value:.0}");
    }
    let decimals = (SIGNIFICANT_DIGITS - 1 - magnitude.log10().floor() as i32).clamp(0, 25);
    let rendered = format!("{value:.*}", decimals as usize);
    let trimmed = if rendered.contains('.') {
        rendered.trim_end_matches('0').trim_end_matches('.')
    } else {
        rendered.as_str()
    };
    if trimmed == "-0" || trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Payload of the status rows ("Fetching exchange rates…") that do nothing
/// when activated.
const STATUS_PAYLOAD: &str = "status";

/// Answers math-like queries and conversions, and copies the result on
/// activation.
pub struct CalculatorPlugin {
    platform: Arc<dyn PlatformProvider>,
    /// Present only when currency conversion is switched on.
    currency: Option<Arc<RateService>>,
}

impl CalculatorPlugin {
    /// Math and unit conversion only; never touches the network.
    pub fn new(platform: Arc<dyn PlatformProvider>) -> Self {
        Self {
            platform,
            currency: None,
        }
    }

    /// Also converts currencies, with rates from `rates`.
    pub fn with_currency(platform: Arc<dyn PlatformProvider>, rates: Arc<RateService>) -> Self {
        Self {
            platform,
            currency: Some(rates),
        }
    }

    /// Currency conversion is on when `[calculator] currency` is true; its
    /// rates are cached in the data directory.
    pub fn from_config(config: &Config, platform: Arc<dyn PlatformProvider>) -> Self {
        if !config.calculator.currency {
            return Self::new(platform);
        }
        match AppPaths::resolve() {
            Ok(paths) => Self::with_currency(
                platform,
                RateService::new(paths.data_dir.join(currency::CACHE_FILE)),
            ),
            Err(err) => {
                tracing::warn!("currency conversion is off: no data directory ({err})");
                Self::new(platform)
            }
        }
    }

    /// A unit or currency conversion for `input`, if it is one.
    fn conversion(&self, input: &str) -> Option<ResultItem> {
        let input = strip_prefix(input);
        if let Some(conversion) = units::answer(input) {
            return Some(Self::copyable(&conversion.text, &conversion.input));
        }
        let rates = self.currency.as_ref()?;
        let answer = currency::answer(input, &rates.availability())?;
        Some(if answer.copyable {
            Self::copyable(&answer.title, &answer.subtitle)
        } else {
            ResultItem::new(
                "calculator",
                "result",
                answer.title,
                Action::Custom {
                    payload: STATUS_PAYLOAD.to_owned(),
                },
            )
            .with_subtitle(answer.subtitle)
            .with_icon(IconSource::builtin("calculator"))
            .with_score(score::EXACT_ANSWER)
        })
    }

    fn copyable(text: &str, detail: &str) -> ResultItem {
        ResultItem::new(
            "calculator",
            "result",
            text,
            Action::CopyText {
                text: text.to_owned(),
            },
        )
        .with_subtitle(format!("{detail} · Enter to copy"))
        .with_icon(IconSource::builtin("calculator"))
        .with_score(score::EXACT_ANSWER)
    }
}

impl Plugin for CalculatorPlugin {
    fn id(&self) -> &str {
        "calculator"
    }

    fn name(&self) -> &str {
        "Calculator"
    }

    fn description(&self) -> &str {
        "Evaluates math expressions and converts units (and currencies, if enabled) as you type; Enter copies the result."
    }

    fn keyword(&self) -> Option<&str> {
        None
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        if let Some(item) = self.conversion(input) {
            return vec![item];
        }
        let Some((expression, value)) = answer(input) else {
            return Vec::new();
        };
        vec![Self::copyable(&format_number(value), &expression)]
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        match &item.action {
            Action::Custom { payload } if payload == STATUS_PAYLOAD => Ok(()),
            action => execute_action(self.platform.as_ref(), action),
        }
    }

    /// Loads the cached exchange rates and, if they are over a day old (or
    /// missing), downloads new ones on a background thread. Returns at once.
    fn refresh(&self) -> PluginResult<()> {
        if let Some(rates) = &self.currency {
            rates.refresh();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn eval(expr: &str) -> f64 {
        evaluate(expr).unwrap_or_else(|e| panic!("`{expr}` failed: {e}"))
    }

    fn assert_close(expr: &str, expected: f64) {
        let actual = eval(expr);
        assert!(
            (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            "`{expr}` = {actual}, expected {expected}"
        );
    }

    fn err(expr: &str) -> CalcError {
        evaluate(expr).expect_err(expr)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("2+2"), 4.0);
        assert_eq!(eval("2 + 3 * 4"), 14.0);
        assert_eq!(eval("(2 + 3) * 4"), 20.0);
        assert_eq!(eval("10 - 4 - 3"), 3.0);
        assert_eq!(eval("100 / 10 / 5"), 2.0);
        assert_eq!(eval("2 * 3 % 4"), 2.0);
        assert_eq!(eval("10 % 4 + 1"), 3.0);
        assert_eq!(eval("7 % 3"), 1.0);
        assert_eq!(eval("-7 % 3"), -1.0);
    }

    #[test]
    fn power_is_right_associative_and_binds_tighter_than_unary_minus() {
        assert_eq!(eval("2^3^2"), 512.0);
        assert_eq!(eval("2**3**2"), 512.0);
        assert_eq!(eval("-2^2"), -4.0);
        assert_eq!(eval("(-2)^2"), 4.0);
        assert_eq!(eval("2^-2"), 0.25);
        assert_eq!(eval("2^10"), 1024.0);
        assert_eq!(eval("2 * 3^2"), 18.0);
        assert_eq!(eval("2^3!"), 64.0);
    }

    #[test]
    fn unary_operators() {
        assert_eq!(eval("-5"), -5.0);
        assert_eq!(eval("+5"), 5.0);
        assert_eq!(eval("--5"), 5.0);
        assert_eq!(eval("-+-5"), 5.0);
        assert_eq!(eval("3 - -2"), 5.0);
        assert_eq!(eval("3*-2"), -6.0);
        assert_eq!(eval("-(2+3)"), -5.0);
    }

    #[test]
    fn number_literals() {
        assert_eq!(eval("1.5 + .5"), 2.0);
        assert_eq!(eval("1e3"), 1000.0);
        assert_eq!(eval("2.5E-3"), 0.0025);
        assert_eq!(eval("1e+2"), 100.0);
        assert_eq!(eval("1_000_000 + 1"), 1_000_001.0);
        assert_eq!(eval("1.+2"), 3.0);
        assert_eq!(eval("2e3*2"), 4000.0);
    }

    #[test]
    fn exponent_marker_is_not_swallowed_without_digits() {
        // `2e` is `2` followed by the constant `e`: no implicit multiplication.
        assert!(matches!(err("2e"), CalcError::UnexpectedToken { .. }));
        assert_close("2*e", 2.0 * std::f64::consts::E);
    }

    #[test]
    fn aliases_and_leading_equals() {
        assert_eq!(eval("6 × 7"), 42.0);
        assert_eq!(eval("84 ÷ 2"), 42.0);
        assert_eq!(eval("= 1 + 1"), 2.0);
        assert_eq!(eval("=1+1"), 2.0);
        assert_eq!(eval("5 − 3"), 2.0);
    }

    #[test]
    fn factorial() {
        assert_eq!(eval("5!"), 120.0);
        assert_eq!(eval("0!"), 1.0);
        assert_eq!(eval("3!!"), 720.0);
        assert_eq!(eval("(2+2)!"), 24.0);
        assert_eq!(eval("2 * 3!"), 12.0);
        assert!(eval("170!").is_finite());
        assert_eq!(err("171!"), CalcError::InvalidFactorial);
        assert_eq!(err("(-1)!"), CalcError::InvalidFactorial);
        assert_eq!(err("2.5!"), CalcError::InvalidFactorial);
    }

    #[test]
    fn functions_with_one_argument() {
        assert_eq!(eval("sqrt(16)"), 4.0);
        assert_eq!(eval("SQRT(16)"), 4.0);
        assert_close("cbrt(27)", 3.0);
        assert_eq!(eval("abs(-3.5)"), 3.5);
        assert_close("sin(pi/2)", 1.0);
        assert_close("cos(0)", 1.0);
        assert_close("tan(0)", 0.0);
        assert_close("asin(1)", std::f64::consts::FRAC_PI_2);
        assert_close("acos(1)", 0.0);
        assert_close("atan(1)", std::f64::consts::FRAC_PI_4);
        assert_close("sinh(0)", 0.0);
        assert_close("cosh(0)", 1.0);
        assert_close("tanh(0)", 0.0);
        assert_close("ln(e)", 1.0);
        assert_close("log(1000)", 3.0);
        assert_close("log2(8)", 3.0);
        assert_close("exp(0)", 1.0);
        assert_eq!(eval("floor(2.7)"), 2.0);
        assert_eq!(eval("floor(-2.1)"), -3.0);
        assert_eq!(eval("ceil(2.1)"), 3.0);
        assert_eq!(eval("round(2.5)"), 3.0);
        assert_eq!(eval("round(-2.5)"), -3.0);
        assert_eq!(eval("sign(-9)"), -1.0);
        assert_eq!(eval("sign(0)"), 0.0);
        assert_eq!(eval("sign(4)"), 1.0);
    }

    #[test]
    fn functions_with_two_arguments() {
        assert_eq!(eval("min(3, 2)"), 2.0);
        assert_eq!(eval("max(3, 2)"), 3.0);
        assert_eq!(eval("pow(2, 10)"), 1024.0);
        assert_eq!(eval("hypot(3, 4)"), 5.0);
        assert_close("atan2(1, 1)", std::f64::consts::FRAC_PI_4);
        assert_eq!(eval("max(1+1, 2*3)"), 6.0);
        assert_eq!(eval("max(min(1, 2), 0)"), 1.0);
    }

    #[test]
    fn constants() {
        assert_close("pi", std::f64::consts::PI);
        assert_close("π", std::f64::consts::PI);
        assert_close("PI", std::f64::consts::PI);
        assert_close("e", std::f64::consts::E);
        assert_close("tau", std::f64::consts::TAU);
        assert_close("2*π", std::f64::consts::TAU);
        assert!(matches!(err("2π"), CalcError::UnexpectedToken { .. }));
    }

    #[test]
    fn errors_are_specific() {
        assert!(matches!(
            err("2 + * 3"),
            CalcError::UnexpectedToken { position: 4, .. }
        ));
        assert_eq!(err("2 +"), CalcError::UnexpectedEnd);
        assert_eq!(err(""), CalcError::UnexpectedEnd);
        assert_eq!(err("(1 + 2"), CalcError::UnexpectedEnd);
        assert!(matches!(
            err("1 + 2)"),
            CalcError::UnexpectedToken { position: 5, .. }
        ));
        assert!(matches!(
            err("foo(2)"),
            CalcError::UnknownFunction { ref name, .. } if name == "foo"
        ));
        assert!(matches!(
            err("2 + foo"),
            CalcError::UnknownConstant { ref name, .. } if name == "foo"
        ));
        assert_eq!(
            err("sqrt(1, 2)"),
            CalcError::WrongArity {
                name: "sqrt".into(),
                expected: 1,
                found: 2
            }
        );
        assert_eq!(
            err("max(1)"),
            CalcError::WrongArity {
                name: "max".into(),
                expected: 2,
                found: 1
            }
        );
        assert_eq!(
            err("sqrt()"),
            CalcError::WrongArity {
                name: "sqrt".into(),
                expected: 1,
                found: 0
            }
        );
        assert!(matches!(
            err("2 $ 3"),
            CalcError::UnexpectedToken { position: 2, .. }
        ));
        assert!(matches!(
            err("1 2"),
            CalcError::UnexpectedToken { position: 2, .. }
        ));
    }

    #[test]
    fn division_by_zero_is_an_error_not_infinity() {
        assert_eq!(err("1/0"), CalcError::DivisionByZero);
        assert_eq!(err("1 / (2 - 2)"), CalcError::DivisionByZero);
        assert_eq!(err("5 % 0"), CalcError::DivisionByZero);
    }

    #[test]
    fn non_finite_results_are_errors() {
        assert_eq!(err("sqrt(-1)"), CalcError::NotFinite);
        assert_eq!(err("10^400"), CalcError::NotFinite);
        assert_eq!(err("ln(0)"), CalcError::NotFinite);
        assert_eq!(err("(-8)^0.5"), CalcError::NotFinite);
    }

    #[test]
    fn deep_nesting_is_rejected_without_overflowing_the_stack() {
        let deep = format!("{}1{}", "(".repeat(5_000), ")".repeat(5_000));
        assert_eq!(err(&deep), CalcError::TooDeeplyNested);
        let signs = format!("{}1", "-".repeat(5_000));
        assert_eq!(err(&signs), CalcError::TooDeeplyNested);
    }

    #[test]
    fn math_like_gate() {
        let answers = |s: &str| answer(s).is_some();
        assert!(answers("2+2"));
        assert!(answers("sqrt(16)"));
        assert!(answers("2^10"));
        assert!(answers("pi*2"));
        assert!(answers("5!"));
        assert!(answers("1 - 1"));
        assert!(answers("= 3 * 3"));
        assert!(answers("abs(-5)"));
        assert!(answers("2 ** 3"));

        assert!(!answers("-5"));
        assert!(!answers("+5"));
        assert!(!answers("42"));
        assert!(!answers("3.14"));
        assert!(!answers("pi"));
        assert!(!answers("e"));
        assert!(!answers("-pi"));
        assert!(!answers("(5)"));
        assert!(!answers(""));
        assert!(!answers("   "));
        assert!(!answers("="));
        assert!(!answers("firefox"));
        assert!(!answers("code - oss"));
        assert!(!answers("7zip"));
        assert!(!answers("2+"));
        assert!(!answers("1/0"));
        assert!(!answers("sqrt(-1)"));
        // A function call without any digit or constant has no value.
        assert!(!answers("abs()"));
    }

    #[test]
    fn normalized_expression() {
        let normalized = |s: &str| answer(s).unwrap().0;
        assert_eq!(normalized("2+2"), "2 + 2");
        assert_eq!(normalized("  = 6×7 "), "6 * 7");
        assert_eq!(normalized("2**3"), "2 ^ 3");
        assert_eq!(normalized("84÷2"), "84 / 2");
        assert_eq!(normalized("3*-2"), "3 * -2");
        assert_eq!(normalized("SQRT(16)+1"), "sqrt(16) + 1");
        assert_eq!(normalized("max(1,2)"), "max(1, 2)");
        assert_eq!(normalized("1_000*2"), "1000 * 2");
        assert_eq!(normalized("5!"), "5!");
    }

    fn shown(expr: &str) -> String {
        format_number(eval(expr))
    }

    #[test]
    fn cancellation_noise_snaps_to_zero() {
        assert_eq!(eval("sin(pi)"), 0.0);
        assert_eq!(eval("cos(pi/2)"), 0.0);
        assert_eq!(eval("tan(pi)"), 0.0);
        assert_eq!(eval("sin(2*pi)"), 0.0);
        assert_eq!(eval("sin(tau)"), 0.0);
        assert_eq!(eval("1 - 0.9 - 0.1"), 0.0);
        assert_eq!(shown("sin(pi)"), "0");
        assert_eq!(shown("cos(pi/2)"), "0");
        assert_eq!(shown("1e6 + 1e-13 - 1e6"), "0");
    }

    #[test]
    fn genuinely_small_results_are_kept() {
        assert_eq!(eval("1e-15"), 1e-15);
        assert_eq!(shown("1e-15"), "1e-15");
        assert_eq!(shown("2e-20*3"), "6e-20");
        assert_eq!(eval("sin(1e-15)"), 1e-15);
        assert_eq!(shown("1e-15 + 2e-15"), "3e-15");
    }

    #[test]
    fn displayed_values_hide_float_noise() {
        assert_eq!(shown("0.1+0.2"), "0.3");
        assert_eq!(shown("1-0.9"), "0.1");
        assert_eq!(shown("0.3*3"), "0.9");
        assert_eq!(shown("sin(pi/2)"), "1");
    }

    #[test]
    fn formatting() {
        assert_eq!(format_number(4.0), "4");
        assert_eq!(format_number(-4.0), "-4");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1.0 / 3.0), "0.333333333333");
        assert_eq!(format_number(2.0 / 3.0), "0.666666666667");
        assert_eq!(format_number(-1.5), "-1.5");
        assert_eq!(format_number(1234.5678), "1234.5678");
        assert_eq!(format_number(0.0025), "0.0025");
        assert_eq!(format_number(123_456_789_012_345.0), "123456789012345");
        assert_eq!(format_number(-123_456_789_012_345.0), "-123456789012345");
        assert_eq!(format_number(std::f64::consts::PI), "3.14159265359");
        assert_eq!(format_number(1.0 / 7.0 * 1000.0), "142.857142857");
        assert_eq!(format_number(9.999_999_999_999_9), "10");
        assert_eq!(format_number(0.000_001_234), "0.000001234");
        assert_eq!(format_number(0.000_000_001), "0.000000001");
    }

    #[test]
    fn scientific_notation_formatting() {
        assert_eq!(format_number(1e15), "1e+15");
        assert_eq!(format_number(1.234e20), "1.234e+20");
        assert_eq!(format_number(-1.234e20), "-1.234e+20");
        assert_eq!(format_number(1e-10), "1e-10");
        assert_eq!(format_number(1.5e-12), "1.5e-12");
        assert_eq!(format_number(-2.5e-10), "-2.5e-10");
        assert_eq!(format_number(2f64.powi(64)), "1.84467440737e+19");
        assert_eq!(format_number(1e300), "1e+300");
    }

    #[test]
    fn never_formats_negative_zero() {
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(-1e-13 + 1e-13), "0");
        assert_eq!(format_number(eval("-0")), "0");
        assert_eq!(format_number(eval("0 * -1")), "0");
    }

    #[test]
    fn plugin_result_shape() {
        let platform = MockPlatform::empty();
        let plugin = CalculatorPlugin::new(platform.clone());
        assert_eq!(plugin.id(), "calculator");
        assert_eq!(plugin.name(), "Calculator");
        assert_eq!(plugin.keyword(), None);
        assert!(plugin.global());

        let results = plugin.query("2+2*3");
        assert_eq!(results.len(), 1);
        let item = &results[0];
        assert_eq!(item.id, "calculator:result");
        assert_eq!(item.title, "8");
        assert_eq!(item.subtitle, "2 + 2 * 3 · Enter to copy");
        assert_eq!(item.icon, Some(IconSource::builtin("calculator")));
        assert_eq!(item.score, score::EXACT_ANSWER);
        assert_eq!(item.action, Action::CopyText { text: "8".into() });
    }

    #[test]
    fn plugin_ignores_non_math() {
        let plugin = CalculatorPlugin::new(MockPlatform::empty());
        assert!(plugin.query("firefox").is_empty());
        assert!(plugin.query("42").is_empty());
        assert!(plugin.query("2+").is_empty());
        assert!(plugin.query("1/0").is_empty());
    }

    #[test]
    fn execute_copies_the_value() {
        let platform = MockPlatform::empty();
        let plugin = CalculatorPlugin::new(platform.clone());
        let results = plugin.query("sqrt(16)");
        plugin.execute(&results[0]).unwrap();
        assert_eq!(*platform.clipboard.lock().unwrap(), vec!["4".to_owned()]);
    }

    #[test]
    fn unit_conversion_row_shape() {
        let platform = MockPlatform::empty();
        let plugin = CalculatorPlugin::new(platform.clone());
        let results = plugin.query("10 km in mi");
        assert_eq!(results.len(), 1);
        let item = &results[0];
        assert_eq!(item.id, "calculator:result");
        assert_eq!(item.title, "6.213711922 mi");
        assert_eq!(item.subtitle, "10 km → mi · Enter to copy");
        assert_eq!(item.icon, Some(IconSource::builtin("calculator")));
        assert_eq!(item.score, score::EXACT_ANSWER);
        assert_eq!(
            item.action,
            Action::CopyText {
                text: "6.213711922 mi".into()
            }
        );
        plugin.execute(item).unwrap();
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            vec!["6.213711922 mi".to_owned()]
        );
    }

    #[test]
    fn unit_conversion_accepts_a_leading_equals_sign() {
        let plugin = CalculatorPlugin::new(MockPlatform::empty());
        assert_eq!(plugin.query("= 100 f to c")[0].title, "37.77777778 °C");
    }

    #[test]
    fn math_still_works_next_to_conversions() {
        let plugin = CalculatorPlugin::new(MockPlatform::empty());
        assert_eq!(plugin.query("2+2*3")[0].title, "8");
        assert!(plugin.query("10 km").is_empty());
        assert!(plugin.query("10 km in kg").is_empty());
        assert!(plugin.query("go to the shop").is_empty());
    }

    const RATES_XML: &str = "<Cube><Cube time='2026-10-02'>\
        <Cube currency='USD' rate='1.1225'/><Cube currency='GBP' rate='0.85033'/>\
        </Cube></Cube>";

    fn currency_plugin(
        platform: Arc<MockPlatform>,
        dir: &tempfile::TempDir,
    ) -> (CalculatorPlugin, Arc<RateService>) {
        let service = RateService::with_fetcher(
            dir.path().join(currency::CACHE_FILE),
            Box::new(|| Ok(RATES_XML.to_owned())),
        );
        (
            CalculatorPlugin::with_currency(platform, service.clone()),
            service,
        )
    }

    #[test]
    fn currency_is_ignored_unless_enabled() {
        let plugin = CalculatorPlugin::new(MockPlatform::empty());
        assert!(plugin.query("100 usd in eur").is_empty());
        let off = CalculatorPlugin::from_config(&Config::default(), MockPlatform::empty());
        assert!(off.query("100 usd in eur").is_empty());
    }

    #[test]
    fn currency_row_shows_the_rate_date_and_copies() {
        let platform = MockPlatform::empty();
        let dir = tempfile::tempdir().unwrap();
        let (plugin, service) = currency_plugin(platform.clone(), &dir);

        // Before any rates exist: a status row that does nothing.
        let waiting = plugin.query("100 usd in eur");
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].title, "Fetching exchange rates…");
        assert_eq!(waiting[0].score, score::EXACT_ANSWER);
        plugin.execute(&waiting[0]).unwrap();
        assert!(platform.clipboard.lock().unwrap().is_empty());

        service.refresh_blocking(1_000);
        let rows = plugin.query("100 usd in eur");
        assert_eq!(rows[0].title, "89.09 EUR");
        assert_eq!(
            rows[0].subtitle,
            "100 usd → eur · 1 USD = 0.890869 EUR · ECB 2026-10-02 · Enter to copy"
        );
        plugin.execute(&rows[0]).unwrap();
        assert_eq!(
            *platform.clipboard.lock().unwrap(),
            vec!["89.09 EUR".to_owned()]
        );
        // Units still win for unit queries, and math is unaffected.
        assert_eq!(plugin.query("1 km in m")[0].title, "1000 m");
        assert_eq!(plugin.query("2+2")[0].title, "4");
        assert!(plugin.query("firefox").is_empty());
    }

    #[test]
    fn refresh_downloads_only_when_currency_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let (plugin, service) = currency_plugin(MockPlatform::empty(), &dir);
        // `refresh` hands the download to a background thread.
        plugin.refresh().unwrap();
        for _ in 0..200 {
            if matches!(service.availability(), currency::Availability::Ready(_)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(matches!(
            service.availability(),
            currency::Availability::Ready(_)
        ));
        assert!(CalculatorPlugin::new(MockPlatform::empty())
            .refresh()
            .is_ok());
    }

    #[test]
    fn status_rows_are_not_unsupported_actions() {
        let plugin = CalculatorPlugin::new(MockPlatform::empty());
        let item = ResultItem::new(
            "calculator",
            "result",
            "x",
            Action::Custom {
                payload: "somebody else's".into(),
            },
        );
        assert!(plugin.execute(&item).is_err());
    }
}
