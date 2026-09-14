(identifier) @variable

((identifier) @type
  (#match? @type "^[A-Z]"))
(primitive_type) @type.builtin
(named_type (identifier) @type)
(type_parameters (identifier) @type)

(keyword) @keyword
["package" "import" "fn" "struct" "enum" "from" "stores" "requires_plain" "as" "mut" "const"] @keyword
(operator) @operator
["->" "&" "*" "!"] @operator
["." "," ":" ";" "::"] @punctuation.delimiter
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
(type_parameters ["<" ">"] @punctuation.bracket)
(type_arguments ["<" ">"] @punctuation.bracket)

(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean
(none) @constant.builtin
[(string_literal) (byte_string_literal) (byte_literal)] @string
(escape_sequence) @string.escape
(comment) @comment

; Adjacent siblings preserve call highlighting through whitespace.
((identifier) @function . (parenthesized_group))
((identifier) @function . "::")
(generic_call function: (identifier) @function)
(function_declaration name: (identifier) @function)
(struct_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(package_clause name: (identifier) @type)
(import_clause alias: (identifier) @type)
(attribute "@" @attribute name: (identifier) @attribute)

((identifier) @property . ":")
("." . (identifier) @property)
("." . (identifier) @function . (parenthesized_group))
(function_declaration
  parameters: (parenthesized_group (identifier) @variable.parameter . ":"))
(enum_declaration body: (block (identifier) @variant))

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))
((identifier) @function
  (#any-of? @function "some" "ok" "err" "assert" "assert_eq" "assert_ne"))
((identifier) @variable.special
  (#eq? @variable.special "self"))
((identifier) @type.builtin
  (#any-of? @type.builtin "Self" "Option" "Result" "MaybeUninit"))
