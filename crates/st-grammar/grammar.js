/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

// IEC 61131-3 Structured Text grammar for tree-sitter.
//
// Case-insensitive keywords: ST is case-insensitive by spec. We handle this
// by defining keyword helpers that match any casing.

/**
 * Create a case-insensitive keyword rule.
 * @param {string} word
 * @returns {SeqRule}
 */
function kw(word) {
  return alias(
    token(
      seq(...word.split("").map((c) =>
        /[a-zA-Z]/.test(c) ? choice(c.toLowerCase(), c.toUpperCase()) : c
      ))
    ),
    word
  );
}

/**
 * Comma-separated list of one or more items.
 */
function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}

/**
 * Comma-separated list of zero or more items.
 */
function commaSep(rule) {
  return optional(commaSep1(rule));
}

module.exports = grammar({
  name: "structured_text",

  extras: ($) => [/\s/, $.line_comment, $.block_comment],

  word: ($) => $.identifier,

  conflicts: ($) => [
    [$.qualified_name, $.variable_access],
    [$.output_assignment, $.variable_access],
    [$.statement_list],
  ],

  rules: {
    // =========================================================================
    // Top-level: a source file is a sequence of declarations
    // =========================================================================
    source_file: ($) =>
      repeat(
        choice(
          $.program_declaration,
          $.function_declaration,
          $.function_block_declaration,
          $.type_declaration,
          $.global_var_declaration
        )
      ),

    // =========================================================================
    // Program Organization Units (POUs)
    // =========================================================================
    program_declaration: ($) =>
      seq(
        kw("PROGRAM"),
        field("name", $.identifier),
        repeat($.var_block),
        field("body", $.statement_list),
        kw("END_PROGRAM")
      ),

    function_declaration: ($) =>
      seq(
        kw("FUNCTION"),
        field("name", $.identifier),
        ":",
        field("return_type", $._data_type),
        repeat($.var_block),
        field("body", $.statement_list),
        kw("END_FUNCTION")
      ),

    function_block_declaration: ($) =>
      seq(
        kw("FUNCTION_BLOCK"),
        field("name", $.identifier),
        repeat($.var_block),
        field("body", $.statement_list),
        kw("END_FUNCTION_BLOCK")
      ),

    // =========================================================================
    // Type declarations
    // =========================================================================
    type_declaration: ($) =>
      seq(
        kw("TYPE"),
        repeat1($.type_definition),
        kw("END_TYPE")
      ),

    type_definition: ($) =>
      seq(
        field("name", $.identifier),
        ":",
        field("type", choice(
          $.struct_type,
          $.enum_type,
          $.subrange_type,
          $._data_type
        )),
        ";"
      ),

    struct_type: ($) =>
      seq(
        kw("STRUCT"),
        repeat1($.struct_field),
        kw("END_STRUCT")
      ),

    struct_field: ($) =>
      seq(
        field("name", $.identifier),
        ":",
        field("type", $._data_type),
        optional(seq(":=", field("default", $._expression))),
        ";"
      ),

    enum_type: ($) =>
      seq(
        "(",
        commaSep1($.enum_value),
        ")"
      ),

    enum_value: ($) =>
      seq(
        field("name", $.identifier),
        optional(seq(":=", field("value", $._literal)))
      ),

    subrange_type: ($) =>
      seq(
        field("base_type", $._elementary_type),
        "(",
        field("lower", $._expression),
        "..",
        field("upper", $._expression),
        ")"
      ),

    array_type: ($) =>
      seq(
        kw("ARRAY"),
        "[",
        commaSep1($.array_range),
        "]",
        kw("OF"),
        field("element_type", $._data_type)
      ),

    array_range: ($) =>
      seq(
        field("lower", $._expression),
        "..",
        field("upper", $._expression)
      ),

    // =========================================================================
    // Variable declaration blocks
    // =========================================================================
    var_block: ($) =>
      seq(
        choice(
          $.var_keyword,
        ),
        repeat($.var_qualifier),
        repeat($.variable_declaration),
        kw("END_VAR")
      ),

    var_keyword: ($) =>
      choice(
        kw("VAR"),
        kw("VAR_INPUT"),
        kw("VAR_OUTPUT"),
        kw("VAR_IN_OUT"),
        kw("VAR_GLOBAL"),
        kw("VAR_EXTERNAL"),
        kw("VAR_TEMP")
      ),

    var_qualifier: ($) =>
      choice(
        kw("RETAIN"),
        kw("PERSISTENT"),
        kw("CONSTANT")
      ),

    variable_declaration: ($) =>
      seq(
        commaSep1(field("name", $.identifier)),
        ":",
        field("type", $._data_type),
        optional(seq(":=", field("initial_value", $._expression))),
        ";"
      ),

    global_var_declaration: ($) =>
      seq(
        kw("VAR_GLOBAL"),
        repeat($.var_qualifier),
        repeat($.variable_declaration),
        kw("END_VAR")
      ),

    // =========================================================================
    // Data types
    // =========================================================================
    _data_type: ($) =>
      choice(
        $._elementary_type,
        $.array_type,
        $.string_type,
        $.ref_type,
        $._type_name
      ),

    _elementary_type: ($) =>
      choice(
        // Boolean
        kw("BOOL"),
        // Signed integers
        kw("SINT"),
        kw("INT"),
        kw("DINT"),
        kw("LINT"),
        // Unsigned integers
        kw("USINT"),
        kw("UINT"),
        kw("UDINT"),
        kw("ULINT"),
        // Floating point
        kw("REAL"),
        kw("LREAL"),
        // Bit strings
        kw("BYTE"),
        kw("WORD"),
        kw("DWORD"),
        kw("LWORD"),
        // Time types
        kw("TIME"),
        kw("LTIME"),
        kw("DATE"),
        kw("LDATE"),
        kw("TIME_OF_DAY"),
        kw("TOD"),
        kw("LTOD"),
        kw("DATE_AND_TIME"),
        kw("DT"),
        kw("LDT")
      ),

    string_type: ($) =>
      seq(
        choice(kw("STRING"), kw("WSTRING")),
        optional(seq("[", field("length", $._expression), "]"))
      ),

    ref_type: ($) =>
      seq(
        kw("REF_TO"),
        field("target_type", $._data_type)
      ),

    // User-defined type name (FB name, struct name, etc.)
    _type_name: ($) => $.qualified_name,

    qualified_name: ($) =>
      seq($.identifier, repeat(seq(".", $.identifier))),

    // =========================================================================
    // Statements
    // =========================================================================
    statement_list: ($) => repeat1($._statement),

    _statement: ($) =>
      choice(
        $.assignment_statement,
        $.function_call_statement,
        $.if_statement,
        $.case_statement,
        $.for_statement,
        $.while_statement,
        $.repeat_statement,
        $.return_statement,
        $.exit_statement,
        $.empty_statement
      ),

    assignment_statement: ($) =>
      seq(
        field("target", $.variable_access),
        ":=",
        field("value", $._expression),
        ";"
      ),

    function_call_statement: ($) =>
      seq($.function_call, ";"),

    if_statement: ($) =>
      seq(
        kw("IF"),
        field("condition", $._expression),
        kw("THEN"),
        field("consequence", optional($.statement_list)),
        repeat($.elsif_clause),
        optional($.else_clause),
        kw("END_IF"),
        ";"
      ),

    elsif_clause: ($) =>
      seq(
        kw("ELSIF"),
        field("condition", $._expression),
        kw("THEN"),
        field("body", optional($.statement_list))
      ),

    else_clause: ($) =>
      seq(
        kw("ELSE"),
        field("body", optional($.statement_list))
      ),

    case_statement: ($) =>
      seq(
        kw("CASE"),
        field("expression", $._expression),
        kw("OF"),
        repeat1($.case_branch),
        optional($.else_clause),
        kw("END_CASE"),
        ";"
      ),

    case_branch: ($) =>
      seq(
        commaSep1($.case_selector),
        ":",
        $.statement_list
      ),

    case_selector: ($) =>
      choice(
        seq($._expression, "..", $._expression),
        $._expression
      ),

    for_statement: ($) =>
      seq(
        kw("FOR"),
        field("variable", $.identifier),
        ":=",
        field("from", $._expression),
        kw("TO"),
        field("to", $._expression),
        optional(seq(kw("BY"), field("step", $._expression))),
        kw("DO"),
        field("body", optional($.statement_list)),
        kw("END_FOR"),
        ";"
      ),

    while_statement: ($) =>
      seq(
        kw("WHILE"),
        field("condition", $._expression),
        kw("DO"),
        field("body", optional($.statement_list)),
        kw("END_WHILE"),
        ";"
      ),

    repeat_statement: ($) =>
      seq(
        kw("REPEAT"),
        field("body", optional($.statement_list)),
        kw("UNTIL"),
        field("condition", $._expression),
        kw("END_REPEAT"),
        ";"
      ),

    return_statement: ($) => seq(kw("RETURN"), ";"),

    exit_statement: ($) => seq(kw("EXIT"), ";"),

    empty_statement: ($) => ";",

    // =========================================================================
    // Expressions — precedence climber
    // =========================================================================
    _expression: ($) =>
      choice(
        $.or_expression,
        $.and_expression,
        $.comparison_expression,
        $.additive_expression,
        $.multiplicative_expression,
        $.power_expression,
        $._unary_expression
      ),

    or_expression: ($) =>
      prec.left(1,
        seq(
          field("left", $._expression),
          field("op", choice(kw("OR"), kw("XOR"))),
          field("right", $._expression)
        )
      ),

    and_expression: ($) =>
      prec.left(2,
        seq(
          field("left", $._expression),
          field("op", choice(kw("AND"), "&")),
          field("right", $._expression)
        )
      ),

    comparison_expression: ($) =>
      prec.left(3,
        seq(
          field("left", $._expression),
          field("op", choice("=", "<>", "<", ">", "<=", ">=")),
          field("right", $._expression)
        )
      ),

    additive_expression: ($) =>
      prec.left(4,
        seq(
          field("left", $._expression),
          field("op", choice("+", "-")),
          field("right", $._expression)
        )
      ),

    multiplicative_expression: ($) =>
      prec.left(5,
        seq(
          field("left", $._expression),
          field("op", choice("*", "/", kw("MOD"))),
          field("right", $._expression)
        )
      ),

    power_expression: ($) =>
      prec.right(6,
        seq(
          field("left", $._expression),
          field("op", "**"),
          field("right", $._expression)
        )
      ),

    _unary_expression: ($) =>
      choice(
        $.unary_expression,
        $._primary_expression
      ),

    unary_expression: ($) =>
      prec(7,
        seq(
          field("op", choice("-", kw("NOT"))),
          field("operand", $._unary_expression)
        )
      ),

    _primary_expression: ($) =>
      choice(
        $._literal,
        $.variable_access,
        $.function_call,
        $.parenthesized_expression
      ),

    parenthesized_expression: ($) =>
      seq("(", $._expression, ")"),

    // =========================================================================
    // Variable access (simple, array indexed, struct field)
    // =========================================================================
    variable_access: ($) =>
      prec.left(8,
        seq(
          $.identifier,
          repeat(
            choice(
              seq(".", $.identifier),
              seq("[", commaSep1($._expression), "]"),
              "^"  // dereference operator
            )
          )
        )
      ),

    // =========================================================================
    // Function / FB calls
    // =========================================================================
    function_call: ($) =>
      prec(9,
        seq(
          field("name", $.qualified_name),
          "(",
          field("arguments", optional($.argument_list)),
          ")"
        )
      ),

    argument_list: ($) =>
      commaSep1(choice($.named_argument, $._expression)),

    named_argument: ($) =>
      seq(
        field("name", $.identifier),
        ":=",
        field("value", $._expression)
      ),

    // Output assignments in FB calls (=>)
    output_assignment: ($) =>
      seq(
        field("name", $.identifier),
        "=>",
        field("target", $.variable_access)
      ),

    // =========================================================================
    // Literals
    // =========================================================================
    _literal: ($) =>
      choice(
        $.integer_literal,
        $.real_literal,
        $.string_literal,
        $.boolean_literal,
        $.null_literal,
        $.time_literal,
        $.date_literal,
        $.tod_literal,
        $.dt_literal,
        $.typed_literal
      ),

    null_literal: ($) => kw("NULL"),

    integer_literal: ($) =>
      token(
        choice(
          // Decimal
          /[0-9][0-9_]*/,
          // Hexadecimal
          /16#[0-9a-fA-F][0-9a-fA-F_]*/,
          // Octal
          /8#[0-7][0-7_]*/,
          // Binary
          /2#[01][01_]*/
        )
      ),

    real_literal: ($) =>
      token(
        seq(
          /[0-9][0-9_]*/,
          ".",
          /[0-9][0-9_]*/,
          optional(seq(/[eE]/, optional(/[+-]/), /[0-9][0-9_]*/))
        )
      ),

    string_literal: ($) =>
      choice(
        // Single-quoted string
        token(seq("'", repeat(choice(/[^'$]/, /\$./)), "'")),
        // Double-quoted wide string
        token(seq('"', repeat(choice(/[^"$]/, /\$./)), '"'))
      ),

    boolean_literal: ($) =>
      choice(kw("TRUE"), kw("FALSE")),

    // T#5s, TIME#1h2m3s, t#100ms
    time_literal: ($) =>
      token(
        seq(
          choice(
            /[tT]#/,
            /[tT][iI][mM][eE]#/,
            /[lL][tT]#/,
            /[lL][tT][iI][mM][eE]#/
          ),
          optional("-"),
          repeat1(
            seq(/[0-9][0-9_]*/, optional("."), optional(/[0-9_]+/), /[a-zA-Z]+/)
          )
        )
      ),

    // D#2024-01-15, DATE#2024-01-15
    date_literal: ($) =>
      token(
        seq(
          choice(
            /[dD]#/,
            /[dD][aA][tT][eE]#/,
            /[lL][dD]#/,
            /[lL][dD][aA][tT][eE]#/
          ),
          /[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}/
        )
      ),

    // TOD#12:30:00, TIME_OF_DAY#08:00:00.123
    tod_literal: ($) =>
      token(
        seq(
          choice(
            /[tT][oO][dD]#/,
            /[tT][iI][mM][eE]_[oO][fF]_[dD][aA][yY]#/,
            /[lL][tT][oO][dD]#/
          ),
          /[0-9]{1,2}:[0-9]{2}:[0-9]{2}/,
          optional(seq(".", /[0-9]+/))
        )
      ),

    // DT#2024-01-15-12:30:00, DATE_AND_TIME#...
    dt_literal: ($) =>
      token(
        seq(
          choice(
            /[dD][tT]#/,
            /[dD][aA][tT][eE]_[aA][nN][dD]_[tT][iI][mM][eE]#/,
            /[lL][dD][tT]#/
          ),
          /[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}/,
          "-",
          /[0-9]{1,2}:[0-9]{2}:[0-9]{2}/,
          optional(seq(".", /[0-9]+/))
        )
      ),

    // Typed literals: INT#5, REAL#3.14, BYTE#16#FF
    typed_literal: ($) =>
      seq(
        field("type", $._elementary_type),
        "#",
        field("value", choice($.integer_literal, $.real_literal))
      ),

    // =========================================================================
    // Comments
    // =========================================================================
    line_comment: ($) => token(seq("//", /.*/)),

    block_comment: ($) =>
      choice(
        $._paren_block_comment,
        $._c_block_comment
      ),

    _paren_block_comment: ($) =>
      token(seq("(*", /(\*[^)]|[^*])*/, "*)")),

    _c_block_comment: ($) =>
      token(seq("/*", /(\*[^/]|[^*])*/, "*/")),

    // =========================================================================
    // Identifier
    // =========================================================================
    identifier: ($) => /[a-zA-Z_][a-zA-Z0-9_]*/,
  },
});
