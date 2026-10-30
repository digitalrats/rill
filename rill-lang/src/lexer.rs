//! Hand-written tokeniser. Produces `Token`s carrying source spans.

use crate::error::{CompileError, Span};
use num_bigint::BigInt;
use num_rational::BigRational;

/// A lexical token kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Numeric literal that contains a `.` or exponent — a fractional literal.
    /// Carries the EXACT value its digits spell plus the source text, so `0.1`
    /// is `1/10` rather than the nearest `f64`.
    FractionalLit(BigRational, String),
    /// Numeric literal with no `.` — an integer. Carries arbitrary precision:
    /// `123456789012345678901234567890` is a valid literal (GHC accepts it as
    /// an `Integer`).
    Int(BigInt),
    /// Identifier / keyword (`sin`, `min`, `process`, user names).
    Ident(String),
    /// String literal, e.g. `"cutoff"`.
    Str(String),
    /// `_`
    Wire,
    /// `!` — a bang marker (`!x`) when attached to its operand, otherwise the
    /// infix indexing operator (`a ! i`).
    Bang,
    /// `:`
    Colon,
    /// `<:`
    Split,
    /// `:>`
    Merge,
    /// `~`
    Tilde,
    /// `@`
    At,
    /// `,`
    Comma,
    /// `+`
    Plus,
    /// `++` — list append (Prelude, `infixr 5`).
    PlusPlus,
    /// `<>` — the "diamond" operator: Semigroup append in Haskell 2010
    /// (`infixr 6`). Named for its shape, which is how it reads in source; the
    /// token also spells the `<>` class METHOD reference.
    Diamond,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `=`
    Eq,
    /// `;`
    Semi,
    /// `where` keyword — optional definition block.
    KwWhere,
    /// `let` keyword — expression-level mutually-recursive bindings.
    KwLet,
    /// `in` keyword — separator in `let defs in expr`.
    KwIn,
    /// `data` keyword — product or sum type declaration.
    KwData,
    /// `type` keyword — type synonym declaration.
    KwType,
    /// `newtype` keyword — distinct wrapper type declaration.
    KwNewtype,
    /// `typeclass` keyword — method dictionary declaration.
    KwTypeclass,
    /// `instance` keyword — concrete typeclass instance.
    KwInstance,
    /// `foreign` keyword — foreign declaration.
    KwForeign,
    /// `forall` keyword — explicit universal quantification in a type.
    KwForall,
    /// `of` keyword — separator in `case x of { .. }`.
    KwOf,
    /// `if` keyword — conditional expression.
    KwIf,
    /// `then` keyword — `if` branch separator.
    KwThen,
    /// `else` keyword — `if` branch separator.
    KwElse,
    /// `->` thin arrow — function-type, lambda, and case-alternative arrow.
    ThinArrow,
    /// `=>` fat arrow — class-constraint arrow (`instance C a => T`).
    FatArrow,
    /// `|` — sum-type constructor separator.
    Pipe,
    /// `.` — field access.
    Dot,
    /// `:=` — field update assignment.
    ColonEq,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `?`
    Question,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `do` keyword — monadic sequencing block.
    KwDo,
    /// `<-` — do-block monadic binding.
    LArrow,
    /// `\` — lambda literal `\x -> body`.
    Backslash,
    /// `` ` `` — backtick infix application (`` a `f` b `` ≡ `f a b`).
    Backtick,
    /// `case` keyword — pattern matching (`case e of { .. }`).
    KwCase,
    /// `::` — type signature annotation (`name :: TypeExpr`).
    DoubleColon,
    /// Newline — layout (offside rule) boundary.
    Newline,
    /// End of input.
    Eof,
    /// Imaginary literal, e.g. `3i`, `2.5i`.
    Imag(f64),
    /// `{-# name value #-}` pragma directive.
    Pragma {
        /// Pragma name, e.g. `MODE`.
        name: String,
        /// Pragma value, e.g. `strict` | `complete`.
        value: String,
    },
}

/// A token plus its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The token kind.
    pub tok: Tok,
    /// Where it came from.
    pub span: Span,
}

/// Allocation budget for the EXACT payload, in DECIMAL digits.
///
/// The budget bounds the WORK a literal may demand — the significand's own
/// digits plus the magnitude of the power of ten that scales it — not the
/// value's magnitude. That distinction is the whole point: an earlier version
/// banded `|value|` at `[10^-1000, 10^1000]`, which silently turned `1e2000`
/// into `10^1000` (1001 numerator digits where GHC's `1e2000 :: Rational` has
/// 2001) even though nothing about `1e2000` is expensive. `10^2000` is 2 KB of
/// decimal digits; only a pathological exponent (`1e4294967296`, which would
/// be 4 GB of digits) has to be bounded.
///
/// So: a literal whose total digit demand fits the budget keeps its EXACT
/// decimal value, whatever its exponent — `1e2000`, `1e-2000` and `1e5000`
/// are all exact. The demand is charged on the REDUCED value: trailing zeros in
/// the mantissa cancel against the scale first, so `1e-99998`, `1.0e-99998` and
/// `10e-99999` are one number with one payload rather than three.
///
/// What happens ABOVE the budget is not uniformly "clamped" — it depends on
/// which side is expensive, and [`decimal_rational`] has the three arms. A value
/// certainly outside `f64`'s range moves to a cheap representative, which
/// changes the AST payload (the residue recorded as OD-F5 in the Task 6 report,
/// owned by the `fromRational` stage). A value whose magnitude is IN `f64`'s
/// range but whose exact payload is large deliberately keeps its MANTISSA exact
/// and leaves the exponent alone: clamping there would change the `f64` image.
/// The image is the same for every clamped literal because clamping only ever
/// happens far outside `f64`'s range (`[5e-324, 1.8e308]`) in the direction of
/// the clamp — that is GHC's behaviour: `1e4294967296 :: Double` is
/// `Infinity`, `0.01e-9223372036854775807 :: Double` is `0.0`.
const EXACT_DIGIT_BUDGET: i64 = 100_000;

/// Decimal magnitude beyond which a value is CERTAINLY outside `f64`'s range.
///
/// `f64`'s finite values live in `[5e-324, 1.8e308]`, so `|value| > 10^400`
/// is always `INFINITY` and `|value| < 10^-400` is always `0.0` — with margin
/// on both sides, so no arithmetic here has to reason about the exact edges of
/// the `f64` range.
const F64_CERTAIN_MAG: i64 = 400;

/// Scale a clamped literal's POWER OF TEN is moved to: still certainly outside
/// `f64`'s range (so the image is unchanged), and cheap.
///
/// This bounds the power, NOT the total payload. The payload a clamped literal
/// materialises is `ndigits + CLAMPED_SCALE` decimal digits, where `ndigits` is
/// the REDUCED mantissa length and therefore bounded only by the input text —
/// `999…9` with 150 000 digits clamps to a 151 000-digit numerator, not to
/// ~1 KB. Making the payload itself small would mean rounding the mantissa,
/// which changes the `f64` image in general; the superlinear cost of a long
/// significand is OD-F3's problem and is proportional to the text either way.
const CLAMPED_SCALE: i64 = 1000;

/// Parse an exponent, SATURATING at `i64`'s bounds.
///
/// The shape is validated first (an optional sign, then at least one ASCII
/// digit), so `1e`, `1e+` and `1e5e5` are still rejected rather than mistaken
/// for a huge exponent. A magnitude beyond `i64` saturates: the value is out
/// of `f64`'s range either way, and the clamp above then decides inf/0.
fn parse_exponent(text: &str) -> Option<i64> {
    let digits = match text.as_bytes().first() {
        Some(b'+') | Some(b'-') => &text[1..],
        _ => text,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(text.parse().unwrap_or(if text.starts_with('-') {
        i64::MIN
    } else {
        i64::MAX
    }))
}

/// The EXACT rational value a fractional literal's decimal text denotes.
///
/// Haskell 2010 §6.4.1: a fractional literal is `fromRational` of the decimal
/// fraction it spells, so this builds the ratio from the DIGITS — never from
/// an `f64`, whose rounding would turn `0.1` into
/// `3602879701896397/36028797018963968`. The grammar accepted here is the
/// Report's `float → decimal . decimal [exponent]` (§2.5), where `decimal` is
/// one or more digits — so BOTH runs are non-empty and `1.` / `1.e3` are not
/// literals (GHC rejects them too) — plus `exponent → (e|E) [+|-] decimal`.
///
/// Every step is checked or saturating: an exponent near `i64`'s bounds
/// saturates, and a literal whose REDUCED payload would demand more than
/// [`EXACT_DIGIT_BUDGET`] decimal digits is bounded (see that constant and the
/// three arms below) rather than overflowing the scale arithmetic or
/// materialising a gigabyte-wide power of ten. The mantissa is reduced against
/// the power of ten FIRST, so the budget charges the value rather than its
/// spelling. Where the payload is altered the change is visible in the AST and
/// recorded as OD-F5 in the Task 6 report; the `f64` image never changes.
///
/// `None` for text the scanner should never have produced; the caller turns it
/// into a lex error rather than a silent value.
pub(crate) fn decimal_rational(text: &str) -> Option<BigRational> {
    let (mantissa, exp) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], parse_exponent(&text[at + 1..])?),
        None => (text, 0),
    };
    let (int_part, frac_part, has_dot) = match mantissa.find('.') {
        Some(at) => (&mantissa[..at], &mantissa[at + 1..], true),
        None => (mantissa, "", false),
    };
    // §2.5 `float → decimal . decimal`: a dot needs digits on BOTH sides.
    if int_part.is_empty() || (has_dot && frac_part.is_empty()) {
        return None;
    }
    // The fraction is `(int ‖ frac) / 10^len(frac)`, scaled by `10^exp`: an
    // exact power of ten in each direction, so no rounding is involved.
    let digits = format!("{int_part}{frac_part}");
    // Leading zeros are not significant digits, and they would corrupt the
    // magnitude below (`0.001` has ONE significant digit, not four).
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Some(BigRational::from_integer(BigInt::from(0)));
    }
    // REDUCE against the power of ten BEFORE the budget is charged. A trailing
    // zero in the mantissa is a factor of ten that cancels exactly against the
    // scale (`10e-99999` and `1e-99998` are ONE number), so leaving it in the
    // significand would charge digits the payload never needs: charging
    // pre-reduction turned `1.0e-99998` into a clamped representative while
    // `1e-99998` stayed exact, giving two payloads for one value. `1.0` is
    // therefore reduced to `1` with `scale` folded up by one.
    let reduced = significant.trim_end_matches('0');
    let stripped = (significant.len() - reduced.len()) as i64;
    // The significand is never all zeros here (`significant` starts with a
    // non-zero digit), so `reduced` is non-empty.
    let numer: BigInt = reduced.parse().ok()?;
    let ndigits = reduced.len() as i64;
    // `saturating_sub`/`saturating_add`: the exponent is already clamped to
    // `i64`, and a literal like `0.01e-9223372036854775807` would otherwise
    // overflow the scale ("attempt to subtract with overflow") through the
    // public API. The same applies to folding `stripped` back in.
    let scale = exp
        .saturating_sub(frac_part.len() as i64)
        .saturating_add(stripped);
    // The exact payload is `numer * 10^scale` (or `numer / 10^-scale`), so the
    // work it demands is the REDUCED significand's digits plus the power's
    // magnitude — NOT the value's magnitude, and not the spelling's digits.
    // Bounding this total is what keeps `1e2000` (2001 digits) and `1e-99998`
    // (99 999 digits) exact while still refusing `1e4294967296` (4 billion).
    let demanded = ndigits.saturating_add(scale.saturating_abs());
    let scale = if demanded <= EXACT_DIGIT_BUDGET {
        scale
    } else {
        // Over budget: this is the ONLY place a literal's exact payload is
        // altered, and only a literal demanding more than 100 000 digits of
        // allocation gets here. `|value|` lies in `[10^(mag-1), 10^mag)`.
        let mag = scale.saturating_add(ndigits);
        if mag > F64_CERTAIN_MAG {
            // Certainly `f64::INFINITY`. Every power beyond 10^400 is
            // indistinguishable in `f64`, so the smallest cheap one is used.
            CLAMPED_SCALE
        } else if mag < -F64_CERTAIN_MAG {
            // Certainly `0.0`: `numer / 10^(1000 + ndigits) < 10^-1000`.
            CLAMPED_SCALE.saturating_add(ndigits).saturating_neg()
        } else {
            // A colossal MANTISSA with an in-range magnitude (`2·10^5` digits
            // times `10^-200000`). Clamping the exponent here would change the
            // `f64` image, and the work is proportional to the input text
            // anyway, so the value stays EXACT. OD-F3 already owns the
            // superlinear cost of a huge significand.
            scale
        }
    };
    // `BigInt::pow` takes a `u32`. In-band the power is bounded by the input's
    // own digit count; only an input too long to address at all can fall out,
    // and that is a lex error rather than a silently truncated exponent.
    let power = u32::try_from(scale.unsigned_abs()).ok()?;
    let ten = BigInt::from(10);
    Some(if scale >= 0 {
        BigRational::from_integer(numer * ten.pow(power))
    } else {
        BigRational::new(numer, ten.pow(power))
    })
}

/// Tokenise `src` into a vector terminated by a single [`Tok::Eof`].
///
/// Whitespace is skipped. `//` starts a line comment.
pub fn tokenize(src: &str) -> Result<Vec<Token>, CompileError> {
    let bytes = src.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    let is_ident_start = |c: u8| c.is_ascii_alphabetic() || c == b'_';
    let is_ident_cont = |c: u8| c.is_ascii_alphanumeric() || c == b'_';

    // Whitespace (except newline) is skipped. `--` starts a line comment. A
    // newline is emitted as `Tok::Newline` so the parser can implement the
    // offside rule (layout) — each line boundary is a layout point.
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\n' {
            out.push(Token {
                tok: Tok::Newline,
                span: Span::new(i, i + 1),
            });
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        // `{-# … #-}` pragma. Matched BEFORE the block-comment path so
        // `{-# MODE complete #-}` is never swallowed as a `{- … -}` comment.
        if c == b'{' && i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'#' {
            // `{-# name value #-}` — scan to the closing `#-}`.
            let mut j = i + 3;
            while j + 2 < bytes.len()
                && !(bytes[j] == b'#' && bytes[j + 1] == b'-' && bytes[j + 2] == b'}')
            {
                j += 1;
            }
            if j + 2 >= bytes.len() {
                return Err(CompileError::Lex {
                    msg: "unterminated pragma (expected `#-}`)".into(),
                    span: Span::new(start, bytes.len()),
                });
            }
            let end = j + 3;
            let content = &src[i + 3..j];
            // Split the inner text on whitespace into exactly `name value`.
            let mut words: Vec<&str> = Vec::new();
            let mut k = i + 3;
            while k < j {
                while k < j && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k >= j {
                    break;
                }
                let wstart = k;
                while k < j && !bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                words.push(&src[wstart..k]);
            }
            if words.len() != 2 {
                return Err(CompileError::Lex {
                    msg: format!("bad pragma `{{-# {content} #-}}` (expected `name value`)"),
                    span: Span::new(start, end),
                });
            }
            out.push(Token {
                tok: Tok::Pragma {
                    name: words[0].to_string(),
                    value: words[1].to_string(),
                },
                span: Span::new(start, end),
            });
            i = end;
            continue;
        }
        // `{- … -}` block comment (Haskell 2010 §2.2), nesting-aware. Only
        // reached for non-pragma `{-`, because the pragma branch above runs
        // first and consumes `{-#`. An unterminated comment is a lex error
        // rather than silently swallowing the rest of the file.
        if c == b'{' && bytes.get(i + 1) == Some(&b'-') {
            let start = i;
            let mut depth = 0usize;
            // A comment that spans a line break still separates the lines it
            // swallowed: without this the definition before the comment and the
            // one after it look like a single layout item.
            let mut line_break: Option<usize> = None;
            while i < bytes.len() {
                if bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'-') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'}') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if bytes[i] == b'\n' && line_break.is_none() {
                        line_break = Some(i);
                    }
                    i += 1;
                }
            }
            if depth != 0 {
                return Err(CompileError::Lex {
                    msg: "unterminated block comment (expected `-}`)".into(),
                    span: Span::new(start, bytes.len()),
                });
            }
            if let Some(nl) = line_break {
                out.push(Token {
                    tok: Tok::Newline,
                    span: Span::new(nl, nl + 1),
                });
            }
            continue;
        }
        if c == b'+' && i + 1 < bytes.len() && bytes[i + 1] == b'+' {
            // `++` — list append. Must precede the single-character `+` so the
            // operator is one token, not two.
            i += 2;
            out.push(Token {
                tok: Tok::PlusPlus,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            // `<>` — Semigroup append. Two characters, so it must be matched
            // before the single-character comparison operators.
            i += 2;
            out.push(Token {
                tok: Tok::Diamond,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            i += 2;
            out.push(Token {
                tok: Tok::Split,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            i += 2;
            out.push(Token {
                tok: Tok::Merge,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            i += 2;
            out.push(Token {
                tok: Tok::DoubleColon,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::ColonEq,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            i += 2;
            out.push(Token {
                tok: Tok::FatArrow,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            i += 2;
            out.push(Token {
                tok: Tok::ThinArrow,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::EqEq,
                span: Span::new(start, i),
            });
            continue;
        }
        // `/=` — Haskell 2010's not-equal. Must be matched before the
        // single-character `/`, and `/=` (not `!=`) is the standard spelling.
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::NotEq,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::NotEq,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::Le,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'>' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::Ge,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'&' && i + 1 < bytes.len() && bytes[i + 1] == b'&' {
            i += 2;
            out.push(Token {
                tok: Tok::AndAnd,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            // `<-` — do-block monadic binding (disjoint from `<:` and `<=`).
            i += 2;
            out.push(Token {
                tok: Tok::LArrow,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'|' && i + 1 < bytes.len() && bytes[i + 1] == b'|' {
            i += 2;
            out.push(Token {
                tok: Tok::OrOr,
                span: Span::new(start, i),
            });
            continue;
        }
        if c.is_ascii_digit() {
            let mut is_float = false;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'e'
                    || bytes[i] == b'E')
            {
                if bytes[i] == b'.' || bytes[i] == b'e' || bytes[i] == b'E' {
                    is_float = true;
                    // A decimal exponent may carry a sign (Haskell 2010 §2.5:
                    // `exponent → (e|E) [+|-] decimal`), so `1e-3` is ONE
                    // literal rather than `1e` followed by `- 3`.
                    if (bytes[i] == b'e' || bytes[i] == b'E')
                        && i + 1 < bytes.len()
                        && (bytes[i + 1] == b'+' || bytes[i + 1] == b'-')
                    {
                        i += 1;
                    }
                }
                i += 1;
            }
            let text = &src[start..i];
            let span = Span::new(start, i);
            if i < bytes.len() && bytes[i] == b'i' {
                i += 1;
                let span = Span::new(start, i);
                let v: f64 = text.parse().map_err(|_| CompileError::Lex {
                    msg: format!("invalid imaginary literal `{text}i`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::Imag(v),
                    span,
                });
            } else if is_float {
                let v = decimal_rational(text).ok_or_else(|| CompileError::Lex {
                    msg: format!("invalid float literal `{text}`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::FractionalLit(v, text.to_string()),
                    span,
                });
            } else {
                let v: BigInt = text.parse().map_err(|_| CompileError::Lex {
                    msg: format!("invalid int literal `{text}`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::Int(v),
                    span,
                });
            }
            continue;
        }
        if is_ident_start(c) {
            while i < bytes.len() && is_ident_cont(bytes[i]) {
                i += 1;
            }
            let text = &src[start..i];
            let span = Span::new(start, i);

            // peek past whitespace to see if `(` follows — if so,
            // `param(`, `keep(`, `inline(` are function calls, not keywords
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let followed_by_paren = j < bytes.len() && bytes[j] == b'(';

            let tok = match text {
                "_" => Tok::Wire,
                "where" if !followed_by_paren => Tok::KwWhere,
                "let" if !followed_by_paren => Tok::KwLet,
                "in" if !followed_by_paren => Tok::KwIn,
                "data" if !followed_by_paren => Tok::KwData,
                "type" if !followed_by_paren => Tok::KwType,
                "newtype" if !followed_by_paren => Tok::KwNewtype,
                // `typeclass` is a DECLARATION keyword only: the
                // `(C a) =>` superclass form puts a `(` right after it, so no
                // call-guard may apply here (unlike `where`/`let`, which are
                // also usable as call heads).
                "typeclass" => Tok::KwTypeclass,
                // `instance` is always a keyword — its head may start with a
                // parenthesized constraint list (`instance (Monad m) => …`), so
                // the `followed_by_paren` call-guard must not apply here.
                "instance" => Tok::KwInstance,
                "foreign" if !followed_by_paren => Tok::KwForeign,
                "forall" if !followed_by_paren => Tok::KwForall,
                // `case` is always a keyword — its scrutinee may start with a
                // parenthesized expression (`case (f x) of …`), so the
                // `followed_by_paren` call-guard must not apply here.
                "case" => Tok::KwCase,
                "of" if !followed_by_paren => Tok::KwOf,
                "if" if !followed_by_paren => Tok::KwIf,
                "then" if !followed_by_paren => Tok::KwThen,
                "else" if !followed_by_paren => Tok::KwElse,
                "do" if !followed_by_paren => Tok::KwDo,
                _ => Tok::Ident(text.to_string()),
            };
            out.push(Token { tok, span });
            continue;
        }
        if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(CompileError::Lex {
                    msg: "unterminated string literal".into(),
                    span: Span::new(start, bytes.len()),
                });
            }
            i += 1;
            let text = src[start + 1..i - 1].to_string();
            out.push(Token {
                tok: Tok::Str(text),
                span: Span::new(start, i),
            });
            continue;
        }
        let single = match c {
            b'\\' => Tok::Backslash,
            b'`' => Tok::Backtick,
            b':' => Tok::Colon,
            b'~' => Tok::Tilde,
            b'@' => Tok::At,
            b',' => Tok::Comma,
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'!' => Tok::Bang,
            b'?' => Tok::Question,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b'<' => Tok::Lt,
            b'>' => Tok::Gt,
            b'=' => Tok::Eq,
            b';' => Tok::Semi,
            b'|' => Tok::Pipe,
            b'.' => Tok::Dot,
            other => {
                return Err(CompileError::Lex {
                    msg: format!("unexpected character `{}`", other as char),
                    span: Span::new(start, start + 1),
                })
            }
        };
        i += 1;
        out.push(Token {
            tok: single,
            span: Span::new(start, i),
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span::new(src.len(), src.len()),
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        tokenize(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    /// A fractional token for `text`, with the exact value its digits spell.
    fn fl(text: &str) -> Tok {
        Tok::FractionalLit(decimal_rational(text).unwrap(), text.to_string())
    }

    #[test]
    fn lexes_combinators_and_ops() {
        assert_eq!(
            kinds("_ : + <: :> ~ @ , * / % ! ? ( ) { } = ;"),
            vec![
                Tok::Wire,
                Tok::Colon,
                Tok::Plus,
                Tok::Split,
                Tok::Merge,
                Tok::Tilde,
                Tok::At,
                Tok::Comma,
                Tok::Star,
                Tok::Slash,
                Tok::Percent,
                Tok::Bang,
                Tok::Question,
                Tok::LParen,
                Tok::RParen,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Eq,
                Tok::Semi,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn distinguishes_int_and_float() {
        assert_eq!(
            kinds("3 3.5 10"),
            vec![Tok::Int(3.into()), fl("3.5"), Tok::Int(10.into()), Tok::Eof]
        );
    }

    #[test]
    fn a_fractional_token_carries_the_exact_value_and_the_source_text() {
        // The four conversions the plan fixes, plus a value whose exact
        // numerator/denominator are both large.
        for (src, n, d) in [
            ("0.1", 1i64, 10i64),
            ("3.14", 157, 50),
            ("1e3", 1000, 1),
            ("1.5e-2", 3, 200),
            ("1.00", 1, 1),
        ] {
            let toks = tokenize(src).unwrap();
            match &toks[0].tok {
                Tok::FractionalLit(v, text) => {
                    assert_eq!(v.numer(), &BigInt::from(n), "numerator of `{src}`");
                    assert_eq!(v.denom(), &BigInt::from(d), "denominator of `{src}`");
                    assert_eq!(text, src, "the source text is kept verbatim");
                }
                other => panic!("expected a fractional token for `{src}`, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_signed_exponent_is_part_of_the_literal() {
        // Haskell 2010 §2.5 allows a sign in `exponent`, so `1e-3` is ONE
        // literal. Read as `1e` minus `3` it would not even tokenise.
        assert_eq!(kinds("1e-3"), vec![fl("1e-3"), Tok::Eof]);
        assert_eq!(kinds("1E+3"), vec![fl("1E+3"), Tok::Eof]);
        assert_eq!(
            fl("1e-3"),
            Tok::FractionalLit(BigRational::new(1.into(), 1000.into()), "1e-3".into())
        );
    }

    #[test]
    fn an_invalid_fractional_literal_is_a_lex_error() {
        // `1e` has no exponent digits; the scanner must not invent a value.
        let err = tokenize("1e").unwrap_err();
        match err {
            CompileError::Lex { msg, .. } => {
                assert!(msg.contains("invalid float literal"), "{msg}")
            }
            other => panic!("expected a lex error, got {other}"),
        }
        // §2.5 `float → decimal . decimal` needs digits on BOTH sides of the
        // dot, so a bare trailing dot is not a float (`1.` would otherwise be
        // `1` followed by unrelated `.` uses, and `1.e3` is not a literal
        // either — GHC rejects both with "Lexical error").
        assert!(tokenize("1.").is_err());
        assert!(tokenize("1.e3").is_err());
    }

    #[test]
    fn an_out_of_range_exponent_clamps_instead_of_rejecting() {
        // An exponent past `f64`'s range is NOT an error: GHC's
        // `1e4294967296 :: Double` is `Infinity`, so the literal keeps a value
        // with the same `f64` image. `1e400` is still `INFINITY` but is now
        // EXACT (401 digits fits the budget), which is the point of budgeting
        // digits rather than banding magnitude.
        for text in ["1e4294967296", "1e9223372036854775808", "1e400"] {
            let toks = tokenize(text).unwrap_or_else(|e| panic!("`{text}` must lex: {e}"));
            match &toks[0].tok {
                Tok::FractionalLit(v, kept) => {
                    assert_eq!(kept, text, "the source spelling is kept");
                    let f = num_traits::ToPrimitive::to_f64(v).unwrap();
                    assert_eq!(f, f64::INFINITY, "`{text}` must be an f64 infinity");
                }
                other => panic!("expected a fractional token for `{text}`, got {other:?}"),
            }
        }
        // ...and the mirror image: a huge NEGATIVE exponent underflows to 0
        // instead of overflowing the scale arithmetic or rejecting.
        for text in [
            "0.01e-9223372036854775807",
            "1e-4294967296",
            "1e-9223372036854775808",
        ] {
            let toks = tokenize(text).unwrap_or_else(|e| panic!("`{text}` must lex: {e}"));
            match &toks[0].tok {
                Tok::FractionalLit(v, _) => {
                    assert_eq!(
                        num_traits::ToPrimitive::to_f64(v).unwrap(),
                        0.0,
                        "`{text}` must underflow to zero"
                    );
                    assert!(v.numer() != &BigInt::from(0), "the mantissa digits survive");
                }
                other => panic!("expected a fractional token for `{text}`, got {other:?}"),
            }
        }
        // Only the ALLOCATION is bounded, and only above the digit budget: a
        // four-billion exponent would need a four-gigabyte power of ten, so it
        // clamps, while a 2000-digit exponent does not (GHC's `1e2000 ::
        // Rational` has 2001 numerator digits — the band this replaced
        // truncated that to 1001).
        let clamped = decimal_rational("1e4294967296").unwrap();
        assert_eq!(
            format!("{}", clamped.numer()).len(),
            1001,
            "the four-billion exponent clamps to the allocation budget"
        );
        assert_eq!(
            format!("{}", decimal_rational("1e2000").unwrap().numer()).len(),
            2001,
            "`1e2000` demands 2001 digits, which the budget allows, so it is EXACT"
        );
        assert_eq!(
            format!("{}", decimal_rational("1e-2000").unwrap().denom()).len(),
            2001,
            "and so is `1e-2000`"
        );
    }

    #[test]
    fn lexes_idents_and_skips_comments() {
        assert_eq!(
            kinds("process -- a comment\n sin"),
            vec![
                Tok::Ident("process".into()),
                Tok::Newline,
                Tok::Ident("sin".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn split_and_merge_are_multichar() {
        assert_eq!(kinds(":>"), vec![Tok::Merge, Tok::Eof]);
        assert_eq!(kinds("<:"), vec![Tok::Split, Tok::Eof]);
    }

    #[test]
    fn rejects_unknown_char() {
        assert!(tokenize("$").is_err());
    }

    #[test]
    fn lexes_string_literal() {
        assert_eq!(
            kinds(r#""cutoff""#),
            vec![Tok::Str("cutoff".into()), Tok::Eof]
        );
    }

    #[test]
    fn rejects_unterminated_string() {
        assert!(tokenize(r#""abc"#).is_err());
    }

    #[test]
    fn lexes_pragma() {
        assert_eq!(
            kinds("{-# MODE complete #-}"),
            vec![
                Tok::Pragma {
                    name: "MODE".into(),
                    value: "complete".into()
                },
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_pragma() {
        assert!(tokenize("{-# MODE complete").is_err());
    }

    #[test]
    fn rejects_malformed_pragma() {
        assert!(tokenize("{-# MODE #-}").is_err());
        assert!(tokenize("{-# MODE complete extra #-}").is_err());
    }

    #[test]
    fn main_is_an_ordinary_identifier() {
        // `main` is no longer a keyword: the entry name is a compile-time
        // pipeline parameter, not a lexer token.
        assert_eq!(
            kinds("main foo bar"),
            vec![
                Tok::Ident("main".into()),
                Tok::Ident("foo".into()),
                Tok::Ident("bar".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn main_is_not_keyword_when_followed_by_paren() {
        assert_eq!(
            kinds(r#"main("freq", 440)"#),
            vec![
                Tok::Ident("main".into()),
                Tok::LParen,
                Tok::Str("freq".into()),
                Tok::Comma,
                Tok::Int(440.into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_let_and_in_keywords() {
        assert_eq!(
            kinds("let x = 1 in x"),
            vec![
                Tok::KwLet,
                Tok::Ident("x".into()),
                Tok::Eq,
                Tok::Int(1.into()),
                Tok::KwIn,
                Tok::Ident("x".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn let_is_not_keyword_when_followed_by_paren() {
        assert_eq!(
            kinds("let(x, y)"),
            vec![
                Tok::Ident("let".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::Comma,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_new_declaration_keywords() {
        assert_eq!(
            kinds("data type newtype typeclass instance foreign of"),
            vec![
                Tok::KwData,
                Tok::KwType,
                Tok::KwNewtype,
                Tok::KwTypeclass,
                Tok::KwInstance,
                Tok::KwForeign,
                Tok::KwOf,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn no_fn_or_match_keywords() {
        // Haskell 2010 surface: `fn` and `match` are ordinary identifiers.
        assert_eq!(
            kinds("fn x"),
            vec![Tok::Ident("fn".into()), Tok::Ident("x".into()), Tok::Eof,]
        );
        assert_eq!(
            kinds("match x"),
            vec![Tok::Ident("match".into()), Tok::Ident("x".into()), Tok::Eof,]
        );
    }

    #[test]
    fn lexes_new_punctuation() {
        assert_eq!(
            kinds("=> := | ."),
            vec![Tok::FatArrow, Tok::ColonEq, Tok::Pipe, Tok::Dot, Tok::Eof]
        );
    }

    #[test]
    fn distinguishes_thin_and_fat_arrow() {
        // Haskell 2010: `->` is the function-type/lambda/case arrow; `=>` is
        // reserved for class constraints.
        assert_eq!(
            kinds("a -> b"),
            vec![
                Tok::Ident("a".into()),
                Tok::ThinArrow,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
        assert_eq!(
            kinds("a => b"),
            vec![
                Tok::Ident("a".into()),
                Tok::FatArrow,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn paren_following_keyword_is_not_a_call_for_other_keywords() {
        // Keywords stay paren-guarded (`data(y)` is a call-style identifier).
        // `match` is no longer a keyword — it lexes as a plain identifier.
        assert_eq!(
            kinds(r#"match(x) data(y)"#),
            vec![
                Tok::Ident("match".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::RParen,
                Tok::Ident("data".into()),
                Tok::LParen,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backslash_lambda_arrow() {
        assert_eq!(
            kinds(r"\x -> x"),
            vec![
                Tok::Backslash,
                Tok::Ident("x".into()),
                Tok::ThinArrow,
                Tok::Ident("x".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_if_then_else_keywords() {
        assert_eq!(
            kinds("if then else"),
            vec![Tok::KwIf, Tok::KwThen, Tok::KwElse, Tok::Eof]
        );
    }

    #[test]
    fn if_then_else_not_keywords_when_followed_by_paren() {
        // `else(`, `if(`, `then(` are call-style identifiers, not keywords.
        assert_eq!(
            kinds("if(x) then(y) else(z)"),
            vec![
                Tok::Ident("if".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::RParen,
                Tok::Ident("then".into()),
                Tok::LParen,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Ident("else".into()),
                Tok::LParen,
                Tok::Ident("z".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_collection_and_comparison_tokens() {
        assert_eq!(
            kinds("[ ] == != < > <= >= && || true false"),
            vec![
                Tok::LBracket,
                Tok::RBracket,
                Tok::EqEq,
                Tok::NotEq,
                Tok::Lt,
                Tok::Gt,
                Tok::Le,
                Tok::Ge,
                Tok::AndAnd,
                Tok::OrOr,
                // `true` / `false` are ordinary IDENTIFIERS: they name the
                // Prelude constructors `True` / `False` (via the parser's
                // alias), not keywords.
                Tok::Ident("true".into()),
                Tok::Ident("false".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn fn_is_not_keyword_when_followed_by_paren() {
        assert_eq!(
            kinds("fn(x, y)"),
            vec![
                Tok::Ident("fn".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::Comma,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backslash_lambda() {
        assert_eq!(
            kinds(r"\x -> x + 1.0"),
            vec![
                Tok::Backslash,
                Tok::Ident("x".into()),
                Tok::ThinArrow,
                Tok::Ident("x".into()),
                Tok::Plus,
                fl("1.0"),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_dash_dash_comment() {
        assert_eq!(
            kinds("1.0 -- a comment\n + 2.0"),
            vec![fl("1.0"), Tok::Newline, Tok::Plus, fl("2.0"), Tok::Eof]
        );
    }

    #[test]
    fn lexes_case_keyword() {
        assert_eq!(
            kinds("case e of { }"),
            vec![
                Tok::KwCase,
                Tok::Ident("e".into()),
                Tok::KwOf,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn case_is_keyword_before_paren() {
        // `case (f x) of …` — a parenthesized scrutinee must not turn the
        // keyword into a call-style identifier.
        assert_eq!(
            kinds("case (C 1.0) of { }"),
            vec![
                Tok::KwCase,
                Tok::LParen,
                Tok::Ident("C".into()),
                fl("1.0"),
                Tok::RParen,
                Tok::KwOf,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_double_colon() {
        assert_eq!(
            kinds("x :: Double"),
            vec![
                Tok::Ident("x".into()),
                Tok::DoubleColon,
                Tok::Ident("Double".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backtick() {
        assert_eq!(
            kinds("1.0 `add` 2.0"),
            vec![
                fl("1.0"),
                Tok::Backtick,
                Tok::Ident("add".into()),
                Tok::Backtick,
                fl("2.0"),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_plusplus_as_one_token() {
        // `++` must not degrade into two `Tok::Plus`, and `+` must still work.
        assert_eq!(
            kinds("a ++ b"),
            vec![
                Tok::Ident("a".into()),
                Tok::PlusPlus,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
        assert_eq!(
            kinds("a + b"),
            vec![
                Tok::Ident("a".into()),
                Tok::Plus,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_nested_block_comment_as_whitespace() {
        assert_eq!(
            kinds("1.0 {- a {- b -} c -} + 2.0"),
            vec![fl("1.0"), Tok::Plus, fl("2.0"), Tok::Eof]
        );
    }

    #[test]
    fn block_comment_keeps_newlines() {
        // A multi-line block comment must not fuse the surrounding lines into
        // one: the newline inside it still separates layout items.
        assert_eq!(
            kinds("x = 1.0; {- a\nb -} y = 2.0;"),
            vec![
                Tok::Ident("x".into()),
                Tok::Eq,
                fl("1.0"),
                Tok::Semi,
                Tok::Newline,
                Tok::Ident("y".into()),
                Tok::Eq,
                fl("2.0"),
                Tok::Semi,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_block_comment() {
        let err = tokenize("{- a {- b -}").unwrap_err();
        match err {
            CompileError::Lex { msg, .. } => {
                assert!(msg.contains("unterminated block comment"), "{msg}")
            }
            other => panic!("expected a lex error, got {other}"),
        }
    }

    #[test]
    fn pragma_is_not_a_block_comment() {
        // `{-# … #-}` is a pragma even though it starts with `{-`.
        assert_eq!(
            kinds("{-# MODE complete #-}\n1.0"),
            vec![
                Tok::Pragma {
                    name: "MODE".into(),
                    value: "complete".into()
                },
                Tok::Newline,
                fl("1.0"),
                Tok::Eof,
            ]
        );
    }
}
