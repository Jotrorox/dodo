// This is an intentionally permissive editor grammar. Declarations and balanced
// groups supply structure; expressions remain token sequences so incomplete code
// and Dodo's newline continuations retain their highlighting. dodo lsp supplies
// syntax and semantic diagnostics using the compiler's authoritative parser.
module.exports = grammar({
  name: 'dodo',

  extras: $ => [/\s/, $.comment],
  word: $ => $.identifier,
  conflicts: $ => [
    [$.named_type, $._token],
    [$.generic_call, $._token],
    [$.generic_call, $.named_type, $._token],
  ],

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.package_clause,
      $.import_clause,
      $.function_declaration,
      $.struct_declaration,
      $.enum_declaration,
      $.attribute,
      $.generic_call,
      $.block,
      $.parenthesized_group,
      $.bracketed_group,
      $._token,
    ),

    package_clause: $ => seq('package', field('name', $.identifier)),
    import_clause: $ => prec.right(seq(
      'import',
      field('path', $.string_literal),
      optional(seq('as', field('alias', $.identifier))),
    )),

    function_declaration: $ => prec.right(seq(
      'fn',
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parenthesized_group),
      optional(seq('->', field('return_type', $._type))),
      optional($.borrow_clause),
      optional($.stores_clause),
      optional($.plain_clause),
      optional(field('body', $.block)),
    )),
    struct_declaration: $ => seq(
      'struct', field('name', $.identifier), optional($.type_parameters),
      field('body', $.block),
    ),
    enum_declaration: $ => seq(
      'enum', field('name', $.identifier), optional($.type_parameters),
      field('body', $.block),
    ),
    type_parameters: $ => seq('<', commaSep1($.identifier), optional(','), '>'),
    borrow_clause: $ => seq('from', $.parenthesized_group),
    stores_clause: $ => seq('stores', $.parenthesized_group),
    plain_clause: $ => seq('requires_plain', $.parenthesized_group),
    attribute: $ => prec.right(seq('@', field('name', $.identifier), optional($.parenthesized_group))),
    generic_call: $ => prec.dynamic(1, seq(
      field('function', $.identifier), optional('::'), $.type_arguments,
      field('arguments', $.parenthesized_group),
    )),

    _type: $ => choice($.named_type, $.reference_type, $.pointer_type, $.array_type, $.slice_type, $.result_type),
    named_type: $ => prec.right(seq(
      choice($.identifier, $.primitive_type),
      repeat(seq('.', $.identifier)),
      optional($.type_arguments),
    )),
    type_arguments: $ => seq('<', commaSep1($._type), optional(','), '>'),
    reference_type: $ => prec(2, seq('&', optional('mut'), $._type)),
    pointer_type: $ => prec(2, seq('*', choice('const', 'mut'), $._type)),
    array_type: $ => prec(2, seq($.bracketed_group, $._type)),
    slice_type: $ => seq('[', $._type, ']'),
    result_type: $ => prec.right(1, seq($._type, '!', $._type)),

    block: $ => seq('{', repeat($._item), '}'),
    parenthesized_group: $ => seq('(', repeat($._item), ')'),
    bracketed_group: $ => seq('[', repeat($._item), ']'),

    _token: $ => choice(
      $.identifier,
      $.primitive_type,
      $.string_literal,
      $.byte_string_literal,
      $.byte_literal,
      $.integer_literal,
      $.float_literal,
      $.boolean_literal,
      $.none,
      $.keyword,
      $.operator,
      '.', ',', ':', ';', '::',
    ),

    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
    primitive_type: _ => choice('bool', 'void', 'str', 'i8', 'i16', 'i32', 'i64', 'isize', 'u8', 'u16', 'u32', 'u64', 'usize', 'f32', 'f64'),
    keyword: _ => choice('pub', 'unsafe', 'extern', 'mut', 'const', 'static', 'let', 'return', 'if', 'else', 'for', 'in', 'break', 'continue', 'match', 'as'),
    boolean_literal: _ => choice('true', 'false'),
    none: _ => 'none',
    operator: _ => choice(
      '<<=', '>>=', '..=', ':=', '->', '=>', '==', '!=', '<=', '>=',
      '&&', '||', '<<', '>>', '+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '..',
      '+', '-', '*', '/', '%', '=', '<', '>', '!', '?', '&', '|', '^', '~',
    ),

    comment: _ => token(seq('//', /[^\n]*/)),
    string_literal: $ => seq('"', repeat(choice(
      token.immediate(prec(1, /[^"\\\r\n]+/)), $.escape_sequence,
    )), token.immediate('"')),
    byte_string_literal: $ => seq('b"', repeat(choice(
      token.immediate(prec(1, /[^"\\\r\n]+/)), $.escape_sequence,
    )), token.immediate('"')),
    byte_literal: $ => seq("b'", choice(
      token.immediate(prec(1, /[^'\\\r\n]/)), $.escape_sequence,
    ), token.immediate("'")),
    escape_sequence: _ => token.immediate(seq('\\', choice(
      /[nrt0\\"']/, /x[0-9a-fA-F]{2}/, /u\{[0-9a-fA-F]{1,6}\}/,
    ))),
    integer_literal: _ => token(seq(
      choice(/0x[0-9a-fA-F_]+/, /0o[0-7_]+/, /0b[01_]+/, /[0-9][0-9_]*/),
      optional(/[iu](8|16|32|64|size)/),
    )),
    float_literal: _ => token(choice(
      seq(/[0-9][0-9_]*/, '.', /[0-9][0-9_]*/, optional(/[eE][+-]?[0-9_]+/), optional(/f(32|64)/)),
      seq(/[0-9][0-9_]*/, /[eE][+-]?[0-9_]+/, optional(/f(32|64)/)),
      seq(/[0-9][0-9_]*/, /f(32|64)/),
    )),
  },
});

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
