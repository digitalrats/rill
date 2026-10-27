# rill-lang: type-level list and tuple syntax (`[a]`, `(a, b)`)

> **Status:** ready to implement.
> **Predecessor:** `2026-10-26-rill-lang-kind-mechanism.md` (K1–K2, `5fdced5`) —
> the kind checker already accepts a kind-`* -> *` constructor applied to one
> argument, so it can check `[a]` as soon as the parser produces it.

## Why

Measured on `316036c`:

| Source | Today | GHC |
|---|---|---|
| `f :: (Int, Double) -> Int` | **accepted** (parser desugars to `Pair`) | accepted |
| `f :: [Int] -> Int` | `parse error: expected type expression, found LBracket` | accepted |
| `f :: [[Int]] -> Int` | same parse error | accepted |
| `f :: [Maybe Int] -> Int` | same parse error | accepted |
| `f :: (Int, [Double]) -> Int` | same parse error | accepted |
| `f :: [] -> Int` (bare list constructor) | parse error | accepted syntax, then a **kind** error (needs one argument) |

Haskell 2010 §3.7 / §4.1.2: `[t]` is built-in syntax for a list type and
`(t1, t2)` for a tuple; `String` is a synonym for `[Char]`. rill spells neither,
so a Haskell 2010 signature cannot be written verbatim.

**Scope note:** this task is **syntax only**. It makes `[a]` parse and produce the
same type representations the nominal spellings already produce. It does **not**
rename `List`→`[]` (task 5.5c), does not add `Char`/`String = [Char]` (5.5b), and
does not touch instance heads or dispatch.

## Design decision: desugar to the existing constructor

`(a, b)` already desugars to `TApp("Pair", [a, b])` (`parser.rs:1312-1322`), and
`types/ffi.rs` matches the AST shape `TApp("List", [_])`. So `[t]` must desugar to
`TApp("List", [t])` and `[]` to `TName("List")`:

- **one** type representation per concept, no parallel `TList`/`TTuple` AST nodes;
- the kind checker (K1) then handles `[Int]` for free — `List` has arity 1;
- the FFI classification keeps working without a second shape to update.

## Steps

- [ ] **Step 1: `[t]` in type position.** In the type parser (`parse_type_single`
      and `parse_type_atom`), accept `Tok::LBracket`, parse one type expression,
      expect `Tok::RBracket`, and desugar to `TApp("List", [t])`. Nested forms
      (`[[Int]]`, `[Maybe Int]`, `(Int, [Double])`) must follow from this rule
      with no special case.
- [ ] **Step 2: `[]` as a type constructor.** `Tok::LBracket` immediately
      followed by `Tok::RBracket` desugars to `TName("List")` — the bare
      constructor, which K1 then rejects in a `*` position with its existing
      message and accepts where a `* -> *` is wanted.
- [ ] **Step 3: `(,)`, `(,,)` are out of scope.** Do not add tuple-constructor
      syntax; record it as a gap if a probe needs it. `(a, b)` already works.
- [ ] **Step 4: The renderer round-trips.** A type that parses from `[Int]` must
      render back as `[Int]`, not `List Int`, or diagnostics will show the
      internal spelling. Check `render.rs` and `TypeEnv::type_spelling`; if the
      renderer prints `List Int` today, make it print `[Int]` and say so.
- [ ] **Step 5: Prove it.** `[Int]`, `[[Int]]`, `[Maybe Int]`, `(Int, [Double])`,
      `[FixedBuffer Double]` parse and type-check; `[]` in a `*` position is a
      **kind** error (not a parse error); an FFI signature written with `[t]`
      classifies the same as the `List t` spelling.
- [ ] **Step 6: No behaviour change** for programs written with the nominal
      spellings. Full suite green.

## Acceptance

- `f :: [Int] -> Int` compiles.
- `f :: [[Int]] -> Int` and `f :: [Maybe Int] -> Int` compile.
- `f :: [] -> Int` is a kind error naming the missing argument (K1's message).
- `[FixedBuffer Double]` compiles — a list of buffers is a list of a kind-`*` type.
- A type printed in a diagnostic shows `[Int]`, not `List Int`.
- All existing tests stay green.
