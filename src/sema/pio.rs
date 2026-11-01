//! `rp_pio.assemble(...)`, the reserved `@compiler(pio)` declaration of
//! std/embedded/chip/rp_pio: its string literal lines are assembled by
//! [`crate::pio`] for the build's chip, and the call becomes a literal of
//! the declaration's `Program` struct before the production checker runs,
//! so programs can initialize constants and cost nothing at run time.
use super::*;
use crate::pio;

const NO_PIO: &str = "PIO programs are assembled for the RP2040 and RP2350 only, and this build targets neither; select a board or chip with PIO, such as board = \"pico\" or \"pico2\" in dodo.toml or --board pico2";

impl Expander {
    pub(super) fn pio_program(
        &self,
        template: &Function,
        type_args: &[Type],
        args: &[Expr],
        span: Span,
    ) -> Check<ExprKind> {
        let version = self
            .pio_version
            .ok_or_else(|| Diagnostic::new(span, NO_PIO))?;
        if !type_args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "rp_pio.assemble takes no type arguments",
            ));
        }
        if args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "rp_pio.assemble takes the program as string literals, one line each",
            ));
        }
        // Each literal holds one or more lines. Remember where each line
        // starts, to point diagnostics at the text in the literal; a literal
        // with escapes points at the whole literal instead.
        let mut source = String::new();
        let mut lines = Vec::new();
        for arg in args {
            let ExprKind::String(bytes, false) = &arg.kind else {
                return Err(Diagnostic::new(
                    arg.span,
                    "rp_pio.assemble takes string literals, one line each, since it assembles the program at compile time",
                ));
            };
            let text = std::str::from_utf8(bytes)
                .map_err(|_| Diagnostic::new(arg.span, "a PIO program must be UTF-8"))?;
            let exact = arg.span.end - arg.span.start == bytes.len() + 2;
            let mut offset = 0;
            for line in text.split('\n') {
                lines.push((arg.span, exact.then_some(arg.span.start + 1 + offset)));
                offset += line.len() + 1;
            }
            source.push_str(text);
            source.push('\n');
        }
        let program = pio::assemble(&source, version).map_err(|error| {
            let span = match lines.get(error.line) {
                Some((_, Some(start))) => Span {
                    start: start + error.start,
                    end: start + error.end.max(error.start + 1),
                },
                Some((literal, None)) => *literal,
                None => span,
            };
            Diagnostic::new(span, format!("invalid PIO program: {}", error.message))
        })?;
        let Type::Named(name) = &template.ret else {
            return Err(Diagnostic::new(span, "rp_pio.assemble must return Program"));
        };
        Ok(program_literal(name, &program, span))
    }
}

/// `rp_pio.Program { ... }`. The fields must match the struct's declaration.
fn program_literal(name: &str, program: &pio::Program, span: Span) -> ExprKind {
    let unsigned = |bits| Type::Int {
        signed: false,
        bits,
    };
    let u16_type = unsigned(16);
    let expr = |kind| Expr::new(kind, span);
    let int = |value: u32| expr(ExprKind::Int(value.into(), Some(unsigned(32))));
    let boolean = |value| expr(ExprKind::Bool(value));
    let mut words = program.instructions.clone();
    words.resize(pio::MAX_INSTRUCTIONS, 0);
    let instructions = words
        .into_iter()
        .map(|word| expr(ExprKind::Int(word.into(), Some(u16_type.clone()))))
        .collect();
    let side_set = program.side_set.unwrap_or(pio::SideSet {
        bits: 0,
        optional: false,
        pindirs: false,
    });
    let fields = [
        (
            "instructions",
            expr(ExprKind::Array(
                Type::Array(pio::MAX_INSTRUCTIONS, Box::new(u16_type.clone())),
                instructions,
            )),
        ),
        ("length", int(program.instructions.len() as u32)),
        ("origin", int(program.origin.unwrap_or(u32::MAX))),
        ("wrap_target", int(program.wrap_target)),
        ("wrap", int(program.wrap)),
        ("side_set_count", int(side_set.count())),
        ("side_set_optional", boolean(side_set.optional)),
        ("side_set_pindirs", boolean(side_set.pindirs)),
        ("version", int(program.version.into())),
        ("clock_divider", int(program.clock_divider)),
        ("fifo", int(program.fifo as u32)),
        ("status_select", int(program.status.select)),
        ("status_n", int(program.status.n)),
        ("in_count", int(program.input.count)),
        ("in_right", boolean(program.input.right)),
        ("autopush", boolean(program.input.auto)),
        ("push_threshold", int(program.input.threshold)),
        ("out_count", int(program.output.count)),
        ("out_right", boolean(program.output.right)),
        ("autopull", boolean(program.output.auto)),
        ("pull_threshold", int(program.output.threshold)),
        ("set_count", int(program.set_count)),
        ("gpio_ranges", int(program.gpio_ranges)),
    ];
    ExprKind::Struct(
        name.to_owned(),
        fields
            .into_iter()
            .map(|(field, value)| (field.to_owned(), value))
            .collect(),
    )
}
