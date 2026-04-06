//! CST (tree-sitter) → AST conversion.
//!
//! Walks the tree-sitter concrete syntax tree and produces typed AST nodes.
//! Parse errors in the CST are collected but do not prevent AST construction
//! for the valid portions of the tree.

use crate::ast::*;
use st_grammar::kind;
use tree_sitter::Node;

/// Errors encountered during lowering.
#[derive(Debug, Clone)]
pub struct LowerError {
    pub message: String,
    pub range: TextRange,
}

/// Result of lowering a source file.
#[derive(Debug)]
pub struct LowerResult {
    pub source_file: SourceFile,
    pub errors: Vec<LowerError>,
}

/// Lower a tree-sitter parse tree into a typed AST.
pub fn lower(tree: &tree_sitter::Tree, source: &str) -> LowerResult {
    let mut ctx = LowerCtx {
        source: source.as_bytes(),
        errors: Vec::new(),
    };
    ctx.collect_cst_errors(tree.root_node());
    let source_file = ctx.lower_source_file(tree.root_node());
    LowerResult {
        source_file,
        errors: ctx.errors,
    }
}

struct LowerCtx<'a> {
    source: &'a [u8],
    errors: Vec<LowerError>,
}

impl<'a> LowerCtx<'a> {
    fn text(&self, node: Node) -> &'a str {
        node.utf8_text(self.source).unwrap_or("")
    }

    fn range(&self, node: Node) -> TextRange {
        TextRange::new(node.start_byte(), node.end_byte())
    }

    fn error(&mut self, msg: impl Into<String>, node: Node) {
        self.errors.push(LowerError {
            message: msg.into(),
            range: self.range(node),
        });
    }

    /// Walk the CST and collect ERROR / MISSING nodes as lowering errors.
    fn collect_cst_errors(&mut self, node: Node) {
        if node.is_error() {
            self.error("syntax error", node);
        } else if node.is_missing() {
            self.error(format!("missing {}", node.kind()), node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.has_error() || child.is_error() || child.is_missing() {
                self.collect_cst_errors(child);
            }
        }
    }

    // =========================================================================
    // Top-level
    // =========================================================================

    fn lower_source_file(&mut self, node: Node) -> SourceFile {
        let mut items = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::PROGRAM_DECLARATION => {
                    items.push(TopLevelItem::Program(self.lower_program(child)));
                }
                kind::FUNCTION_DECLARATION => {
                    items.push(TopLevelItem::Function(self.lower_function(child)));
                }
                kind::FUNCTION_BLOCK_DECLARATION => {
                    items.push(TopLevelItem::FunctionBlock(
                        self.lower_function_block(child),
                    ));
                }
                kind::TYPE_DECLARATION => {
                    items.push(TopLevelItem::TypeDeclaration(
                        self.lower_type_declaration(child),
                    ));
                }
                kind::GLOBAL_VAR_DECLARATION => {
                    items.push(TopLevelItem::GlobalVarDeclaration(
                        self.lower_global_var_block(child),
                    ));
                }
                _ if child.is_error() => {
                    self.error("syntax error", child);
                }
                _ => {}
            }
        }
        SourceFile {
            items,
            range: self.range(node),
        }
    }

    // =========================================================================
    // POUs
    // =========================================================================

    fn lower_program(&mut self, node: Node) -> ProgramDecl {
        let name = self.field_identifier(node, "name");
        let var_blocks = self.collect_var_blocks(node);
        let body = self.field_statements(node, "body");
        ProgramDecl {
            name,
            var_blocks,
            body,
            range: self.range(node),
        }
    }

    fn lower_function(&mut self, node: Node) -> FunctionDecl {
        let name = self.field_identifier(node, "name");
        let return_type = node
            .child_by_field_name("return_type")
            .map(|n| self.lower_data_type(n))
            .unwrap_or(DataType::Elementary(ElementaryType::Int));
        let var_blocks = self.collect_var_blocks(node);
        let body = self.field_statements(node, "body");
        FunctionDecl {
            name,
            return_type,
            var_blocks,
            body,
            range: self.range(node),
        }
    }

    fn lower_function_block(&mut self, node: Node) -> FunctionBlockDecl {
        let name = self.field_identifier(node, "name");
        let var_blocks = self.collect_var_blocks(node);
        let body = self.field_statements(node, "body");
        FunctionBlockDecl {
            name,
            var_blocks,
            body,
            range: self.range(node),
        }
    }

    // =========================================================================
    // Type declarations
    // =========================================================================

    fn lower_type_declaration(&mut self, node: Node) -> TypeDeclarationBlock {
        let mut definitions = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind::TYPE_DEFINITION {
                definitions.push(self.lower_type_definition(child));
            }
        }
        TypeDeclarationBlock {
            definitions,
            range: self.range(node),
        }
    }

    fn lower_type_definition(&mut self, node: Node) -> TypeDefinition {
        let name = self.field_identifier(node, "name");
        let ty_node = node.child_by_field_name("type");
        let ty = match ty_node {
            Some(n) => match n.kind() {
                kind::STRUCT_TYPE => TypeDefKind::Struct(self.lower_struct_type(n)),
                kind::ENUM_TYPE => TypeDefKind::Enum(self.lower_enum_type(n)),
                kind::SUBRANGE_TYPE => TypeDefKind::Subrange(self.lower_subrange_type(n)),
                _ => TypeDefKind::Alias(self.lower_data_type(n)),
            },
            None => {
                self.error("missing type in type definition", node);
                TypeDefKind::Alias(DataType::Elementary(ElementaryType::Int))
            }
        };
        TypeDefinition {
            name,
            ty,
            range: self.range(node),
        }
    }

    fn lower_struct_type(&mut self, node: Node) -> StructType {
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind::STRUCT_FIELD {
                fields.push(self.lower_struct_field(child));
            }
        }
        StructType {
            fields,
            range: self.range(node),
        }
    }

    fn lower_struct_field(&mut self, node: Node) -> StructField {
        let name = self.field_identifier(node, "name");
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_data_type(n))
            .unwrap_or(DataType::Elementary(ElementaryType::Int));
        let default = node
            .child_by_field_name("default")
            .map(|n| self.lower_expression(n));
        StructField {
            name,
            ty,
            default,
            range: self.range(node),
        }
    }

    fn lower_enum_type(&mut self, node: Node) -> EnumType {
        let mut values = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind::ENUM_VALUE {
                values.push(self.lower_enum_value(child));
            }
        }
        EnumType {
            values,
            range: self.range(node),
        }
    }

    fn lower_enum_value(&mut self, node: Node) -> EnumValue {
        let name = self.field_identifier(node, "name");
        let value = node.child_by_field_name("value").and_then(|n| {
            if let Expression::Literal(lit) = self.lower_expression(n) {
                Some(lit)
            } else {
                None
            }
        });
        EnumValue {
            name,
            value,
            range: self.range(node),
        }
    }

    fn lower_subrange_type(&mut self, node: Node) -> SubrangeType {
        let base_type = node
            .child_by_field_name("base_type")
            .map(|n| self.parse_elementary_type(self.text(n)))
            .unwrap_or(ElementaryType::Int);
        let lower = node
            .child_by_field_name("lower")
            .map(|n| self.lower_expression(n))
            .unwrap_or(Expression::Literal(Literal {
                kind: LiteralKind::Integer(0),
                range: self.range(node),
            }));
        let upper = node
            .child_by_field_name("upper")
            .map(|n| self.lower_expression(n))
            .unwrap_or(Expression::Literal(Literal {
                kind: LiteralKind::Integer(0),
                range: self.range(node),
            }));
        SubrangeType {
            base_type,
            lower,
            upper,
            range: self.range(node),
        }
    }

    // =========================================================================
    // Variable blocks
    // =========================================================================

    fn collect_var_blocks(&mut self, parent: Node) -> Vec<VarBlock> {
        let mut blocks = Vec::new();
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            if child.kind() == kind::VAR_BLOCK {
                blocks.push(self.lower_var_block(child));
            }
        }
        blocks
    }

    fn lower_var_block(&mut self, node: Node) -> VarBlock {
        let mut kind_val = VarKind::Var;
        let mut qualifiers = Vec::new();
        let mut declarations = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::VAR_KEYWORD => {
                    kind_val = self.parse_var_kind(self.text(child));
                }
                kind::VAR_QUALIFIER => {
                    if let Some(q) = self.parse_var_qualifier(self.text(child)) {
                        qualifiers.push(q);
                    }
                }
                kind::VARIABLE_DECLARATION => {
                    declarations.push(self.lower_var_declaration(child));
                }
                _ => {}
            }
        }
        VarBlock {
            kind: kind_val,
            qualifiers,
            declarations,
            range: self.range(node),
        }
    }

    fn lower_global_var_block(&mut self, node: Node) -> VarBlock {
        let mut qualifiers = Vec::new();
        let mut declarations = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::VAR_QUALIFIER => {
                    if let Some(q) = self.parse_var_qualifier(self.text(child)) {
                        qualifiers.push(q);
                    }
                }
                kind::VARIABLE_DECLARATION => {
                    declarations.push(self.lower_var_declaration(child));
                }
                _ => {}
            }
        }
        VarBlock {
            kind: VarKind::VarGlobal,
            qualifiers,
            declarations,
            range: self.range(node),
        }
    }

    fn lower_var_declaration(&mut self, node: Node) -> VarDeclaration {
        let mut names = Vec::new();
        let mut ty = DataType::Elementary(ElementaryType::Int);
        let mut initial_value = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind::IDENTIFIER
                && node.field_name_for_child(child.id() as u32) == Some("name")
            {
                names.push(self.lower_identifier(child));
            }
        }

        // Fallback: if field-based approach didn't find names, scan named children
        if names.is_empty() {
            let mut cursor2 = node.walk();
            for child in node.named_children(&mut cursor2) {
                if child.kind() == kind::IDENTIFIER {
                    names.push(self.lower_identifier(child));
                }
            }
        }

        if let Some(ty_node) = node.child_by_field_name("type") {
            ty = self.lower_data_type(ty_node);
        }
        if let Some(init_node) = node.child_by_field_name("initial_value") {
            initial_value = Some(self.lower_expression(init_node));
        }

        VarDeclaration {
            names,
            ty,
            initial_value,
            range: self.range(node),
        }
    }

    // =========================================================================
    // Data types
    // =========================================================================

    fn lower_data_type(&mut self, node: Node) -> DataType {
        match node.kind() {
            kind::ARRAY_TYPE => DataType::Array(Box::new(self.lower_array_type(node))),
            kind::STRING_TYPE => DataType::String(self.lower_string_type(node)),
            kind::REF_TYPE => {
                let target = node.child_by_field_name("target_type")
                    .map(|n| self.lower_data_type(n))
                    .unwrap_or(DataType::Elementary(ElementaryType::Int));
                DataType::Ref(Box::new(target))
            }
            kind::QUALIFIED_NAME => DataType::UserDefined(self.lower_qualified_name(node)),
            // Elementary type keywords show up as anonymous nodes with text like "INT", "BOOL"
            _ => {
                let text = self.text(node);
                if let Some(et) = self.try_parse_elementary_type(text) {
                    DataType::Elementary(et)
                } else if node.kind() == kind::IDENTIFIER {
                    // Single identifier used as type name
                    DataType::UserDefined(QualifiedName {
                        parts: vec![self.lower_identifier(node)],
                        range: self.range(node),
                    })
                } else {
                    // Try to find a named child that is a qualified_name or elementary type
                    self.lower_data_type_from_children(node)
                }
            }
        }
    }

    fn lower_data_type_from_children(&mut self, node: Node) -> DataType {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                kind::ARRAY_TYPE => return DataType::Array(Box::new(self.lower_array_type(child))),
                kind::STRING_TYPE => return DataType::String(self.lower_string_type(child)),
                kind::QUALIFIED_NAME => {
                    return DataType::UserDefined(self.lower_qualified_name(child));
                }
                _ => {
                    let text = self.text(child);
                    if let Some(et) = self.try_parse_elementary_type(text) {
                        return DataType::Elementary(et);
                    }
                }
            }
        }
        // Last resort: try the node text directly
        DataType::Elementary(self.parse_elementary_type(self.text(node)))
    }

    fn lower_array_type(&mut self, node: Node) -> ArrayType {
        let mut ranges = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind::ARRAY_RANGE {
                ranges.push(self.lower_array_range(child));
            }
        }
        let element_type = node
            .child_by_field_name("element_type")
            .map(|n| self.lower_data_type(n))
            .unwrap_or(DataType::Elementary(ElementaryType::Int));
        ArrayType {
            ranges,
            element_type,
            range: self.range(node),
        }
    }

    fn lower_array_range(&mut self, node: Node) -> ArrayRange {
        let lower = node
            .child_by_field_name("lower")
            .map(|n| self.lower_expression(n))
            .unwrap_or(Expression::Literal(Literal {
                kind: LiteralKind::Integer(0),
                range: self.range(node),
            }));
        let upper = node
            .child_by_field_name("upper")
            .map(|n| self.lower_expression(n))
            .unwrap_or(Expression::Literal(Literal {
                kind: LiteralKind::Integer(0),
                range: self.range(node),
            }));
        ArrayRange {
            lower,
            upper,
            range: self.range(node),
        }
    }

    fn lower_string_type(&mut self, node: Node) -> StringType {
        let text = self.text(node).to_uppercase();
        let wide = text.starts_with("WSTRING");
        let length = node
            .child_by_field_name("length")
            .map(|n| self.lower_expression(n));
        StringType {
            wide,
            length,
            range: self.range(node),
        }
    }

    // =========================================================================
    // Statements
    // =========================================================================

    fn field_statements(&mut self, node: Node, field: &str) -> Vec<Statement> {
        match node.child_by_field_name(field) {
            Some(sl) if sl.kind() == kind::STATEMENT_LIST => self.lower_statement_list(sl),
            _ => Vec::new(),
        }
    }

    fn lower_statement_list(&mut self, node: Node) -> Vec<Statement> {
        let mut stmts = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if let Some(stmt) = self.lower_statement(child) {
                stmts.push(stmt);
            }
        }
        stmts
    }

    fn lower_statement(&mut self, node: Node) -> Option<Statement> {
        match node.kind() {
            kind::ASSIGNMENT_STATEMENT => Some(Statement::Assignment(self.lower_assignment(node))),
            kind::FUNCTION_CALL_STATEMENT => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == kind::FUNCTION_CALL {
                        return Some(Statement::FunctionCall(self.lower_function_call(child)));
                    }
                }
                None
            }
            kind::IF_STATEMENT => Some(Statement::If(self.lower_if_stmt(node))),
            kind::CASE_STATEMENT => Some(Statement::Case(self.lower_case_stmt(node))),
            kind::FOR_STATEMENT => Some(Statement::For(self.lower_for_stmt(node))),
            kind::WHILE_STATEMENT => Some(Statement::While(self.lower_while_stmt(node))),
            kind::REPEAT_STATEMENT => Some(Statement::Repeat(self.lower_repeat_stmt(node))),
            kind::RETURN_STATEMENT => Some(Statement::Return(self.range(node))),
            kind::EXIT_STATEMENT => Some(Statement::Exit(self.range(node))),
            kind::EMPTY_STATEMENT => Some(Statement::Empty(self.range(node))),
            _ if node.is_error() => {
                self.error("syntax error in statement", node);
                None
            }
            _ => None,
        }
    }

    fn lower_assignment(&mut self, node: Node) -> AssignmentStmt {
        let target = node
            .child_by_field_name("target")
            .map(|n| self.lower_variable_access(n))
            .unwrap_or_else(|| self.dummy_variable_access(node));
        let value = node
            .child_by_field_name("value")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        AssignmentStmt {
            target,
            value,
            range: self.range(node),
        }
    }

    fn lower_if_stmt(&mut self, node: Node) -> IfStmt {
        let condition = node
            .child_by_field_name("condition")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let then_body = self.field_statements(node, "consequence");
        let mut elsif_clauses = Vec::new();
        let mut else_body = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::ELSIF_CLAUSE => {
                    elsif_clauses.push(self.lower_elsif_clause(child));
                }
                kind::ELSE_CLAUSE => {
                    else_body = Some(self.lower_else_body(child));
                }
                _ => {}
            }
        }
        IfStmt {
            condition,
            then_body,
            elsif_clauses,
            else_body,
            range: self.range(node),
        }
    }

    fn lower_elsif_clause(&mut self, node: Node) -> ElsifClause {
        let condition = node
            .child_by_field_name("condition")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let body = self.field_statements(node, "body");
        ElsifClause {
            condition,
            body,
            range: self.range(node),
        }
    }

    fn lower_else_body(&mut self, node: Node) -> Vec<Statement> {
        self.field_statements(node, "body")
    }

    fn lower_case_stmt(&mut self, node: Node) -> CaseStmt {
        let expression = node
            .child_by_field_name("expression")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let mut branches = Vec::new();
        let mut else_body = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::CASE_BRANCH => branches.push(self.lower_case_branch(child)),
                kind::ELSE_CLAUSE => else_body = Some(self.lower_else_body(child)),
                _ => {}
            }
        }
        CaseStmt {
            expression,
            branches,
            else_body,
            range: self.range(node),
        }
    }

    fn lower_case_branch(&mut self, node: Node) -> CaseBranch {
        let mut selectors = Vec::new();
        let mut body = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::CASE_SELECTOR => selectors.push(self.lower_case_selector(child)),
                kind::STATEMENT_LIST => body = self.lower_statement_list(child),
                _ => {}
            }
        }
        CaseBranch {
            selectors,
            body,
            range: self.range(node),
        }
    }

    fn lower_case_selector(&mut self, node: Node) -> CaseSelector {
        // A case selector is either a single expression or a range (expr..expr)
        let named_count = node.named_child_count();
        if named_count >= 2 {
            let lower = self.lower_expression(node.named_child(0).unwrap());
            let upper = self.lower_expression(node.named_child(1).unwrap());
            CaseSelector::Range(lower, upper)
        } else if named_count == 1 {
            CaseSelector::Single(self.lower_expression(node.named_child(0).unwrap()))
        } else {
            CaseSelector::Single(self.dummy_expression(node))
        }
    }

    fn lower_for_stmt(&mut self, node: Node) -> ForStmt {
        let variable = node
            .child_by_field_name("variable")
            .map(|n| self.lower_identifier(n))
            .unwrap_or_else(|| self.dummy_identifier(node));
        let from = node
            .child_by_field_name("from")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let to = node
            .child_by_field_name("to")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let by = node
            .child_by_field_name("step")
            .map(|n| self.lower_expression(n));
        let body = self.field_statements(node, "body");
        ForStmt {
            variable,
            from,
            to,
            by,
            body,
            range: self.range(node),
        }
    }

    fn lower_while_stmt(&mut self, node: Node) -> WhileStmt {
        let condition = node
            .child_by_field_name("condition")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let body = self.field_statements(node, "body");
        WhileStmt {
            condition,
            body,
            range: self.range(node),
        }
    }

    fn lower_repeat_stmt(&mut self, node: Node) -> RepeatStmt {
        let body = self.field_statements(node, "body");
        let condition = node
            .child_by_field_name("condition")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        RepeatStmt {
            body,
            condition,
            range: self.range(node),
        }
    }

    // =========================================================================
    // Expressions
    // =========================================================================

    fn lower_expression(&mut self, node: Node) -> Expression {
        match node.kind() {
            kind::INTEGER_LITERAL => Expression::Literal(self.lower_integer_literal(node)),
            kind::REAL_LITERAL => Expression::Literal(self.lower_real_literal(node)),
            kind::STRING_LITERAL => Expression::Literal(self.lower_string_literal(node)),
            kind::BOOLEAN_LITERAL => Expression::Literal(self.lower_boolean_literal(node)),
            kind::TIME_LITERAL => Expression::Literal(Literal {
                kind: LiteralKind::Time(self.text(node).to_string()),
                range: self.range(node),
            }),
            kind::DATE_LITERAL => Expression::Literal(Literal {
                kind: LiteralKind::Date(self.text(node).to_string()),
                range: self.range(node),
            }),
            kind::TOD_LITERAL => Expression::Literal(Literal {
                kind: LiteralKind::Tod(self.text(node).to_string()),
                range: self.range(node),
            }),
            kind::DT_LITERAL => Expression::Literal(Literal {
                kind: LiteralKind::Dt(self.text(node).to_string()),
                range: self.range(node),
            }),
            kind::TYPED_LITERAL => Expression::Literal(self.lower_typed_literal(node)),
            kind::NULL_LITERAL => Expression::Literal(Literal {
                kind: LiteralKind::Null,
                range: self.range(node),
            }),
            kind::VARIABLE_ACCESS => Expression::Variable(self.lower_variable_access(node)),
            kind::FUNCTION_CALL => {
                Expression::FunctionCall(Box::new(self.lower_function_call(node)))
            }
            kind::PARENTHESIZED_EXPRESSION => {
                let inner = node
                    .named_child(0)
                    .map(|n| self.lower_expression(n))
                    .unwrap_or_else(|| self.dummy_expression(node));
                Expression::Parenthesized(Box::new(inner))
            }
            kind::UNARY_EXPRESSION => self.lower_unary_expression(node),
            kind::OR_EXPRESSION
            | kind::AND_EXPRESSION
            | kind::COMPARISON_EXPRESSION
            | kind::ADDITIVE_EXPRESSION
            | kind::MULTIPLICATIVE_EXPRESSION
            | kind::POWER_EXPRESSION => self.lower_binary_expression(node),
            _ if node.is_error() => {
                self.error("syntax error in expression", node);
                self.dummy_expression(node)
            }
            _ => {
                // Might be an anonymous node wrapping something else
                if let Some(child) = node.named_child(0) {
                    self.lower_expression(child)
                } else {
                    self.dummy_expression(node)
                }
            }
        }
    }

    fn lower_unary_expression(&mut self, node: Node) -> Expression {
        let op_text = node
            .child_by_field_name("op")
            .map(|n| self.text(n).to_uppercase())
            .unwrap_or_default();
        let op = match op_text.as_str() {
            "NOT" => UnaryOp::Not,
            _ => UnaryOp::Neg,
        };
        let operand = node
            .child_by_field_name("operand")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        Expression::Unary(Box::new(UnaryExpr {
            op,
            operand,
            range: self.range(node),
        }))
    }

    fn lower_binary_expression(&mut self, node: Node) -> Expression {
        let op_node = node.child_by_field_name("op");
        let op_text = op_node
            .map(|n| self.text(n).to_uppercase())
            .unwrap_or_default();
        let op = match op_text.as_str() {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "MOD" => BinaryOp::Mod,
            "**" => BinaryOp::Power,
            "AND" | "&" => BinaryOp::And,
            "OR" => BinaryOp::Or,
            "XOR" => BinaryOp::Xor,
            "=" => BinaryOp::Eq,
            "<>" => BinaryOp::Ne,
            "<" => BinaryOp::Lt,
            ">" => BinaryOp::Gt,
            "<=" => BinaryOp::Le,
            ">=" => BinaryOp::Ge,
            _ => BinaryOp::Add,
        };
        let left = node
            .child_by_field_name("left")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        let right = node
            .child_by_field_name("right")
            .map(|n| self.lower_expression(n))
            .unwrap_or_else(|| self.dummy_expression(node));
        Expression::Binary(Box::new(BinaryExpr {
            op,
            left,
            right,
            range: self.range(node),
        }))
    }

    fn lower_variable_access(&mut self, node: Node) -> VariableAccess {
        let mut parts = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                kind::IDENTIFIER => parts.push(AccessPart::Identifier(self.lower_identifier(child))),
                _ if self.text(child) == "^" => {
                    parts.push(AccessPart::Deref);
                }
                _ if child.kind().contains("expression") || child.kind() == kind::INTEGER_LITERAL || child.kind() == kind::VARIABLE_ACCESS => {
                    // Array index — handled below
                }
                _ => {}
            }
        }

        // Handle array indexing: look for bracket-delimited expression groups
        let full_text = self.text(node);
        if full_text.contains('[') {
            // Re-walk to find indexed accesses properly
            parts.clear();
            let mut cursor2 = node.walk();
            let children: Vec<_> = node.children(&mut cursor2).collect();
            let mut i = 0;
            while i < children.len() {
                let child = children[i];
                if child.kind() == kind::IDENTIFIER {
                    parts.push(AccessPart::Identifier(self.lower_identifier(child)));
                } else if self.text(child) == "[" {
                    let mut indices = Vec::new();
                    i += 1;
                    while i < children.len() && self.text(children[i]) != "]" {
                        let c = children[i];
                        if c.is_named() && self.text(c) != "," {
                            indices.push(self.lower_expression(c));
                        }
                        i += 1;
                    }
                    parts.push(AccessPart::Index(indices));
                } else if self.text(child) == "." {
                    // dot separator, skip
                }
                i += 1;
            }
        }

        VariableAccess {
            parts,
            range: self.range(node),
        }
    }

    fn lower_function_call(&mut self, node: Node) -> FunctionCallExpr {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.lower_qualified_name(n))
            .unwrap_or_else(|| QualifiedName {
                parts: vec![self.dummy_identifier(node)],
                range: self.range(node),
            });
        let arguments = node
            .child_by_field_name("arguments")
            .map(|n| self.lower_argument_list(n))
            .unwrap_or_default();
        FunctionCallExpr {
            name,
            arguments,
            range: self.range(node),
        }
    }

    fn lower_argument_list(&mut self, node: Node) -> Vec<Argument> {
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                kind::NAMED_ARGUMENT => {
                    let name = self.field_identifier(child, "name");
                    let value = child
                        .child_by_field_name("value")
                        .map(|n| self.lower_expression(n))
                        .unwrap_or_else(|| self.dummy_expression(child));
                    args.push(Argument::Named { name, value });
                }
                _ => {
                    args.push(Argument::Positional(self.lower_expression(child)));
                }
            }
        }
        args
    }

    // =========================================================================
    // Literals
    // =========================================================================

    fn lower_integer_literal(&mut self, node: Node) -> Literal {
        let text = self.text(node).replace('_', "");
        let value = if let Some(hex) = text.strip_prefix("16#") {
            i64::from_str_radix(hex, 16).unwrap_or(0)
        } else if let Some(oct) = text.strip_prefix("8#") {
            i64::from_str_radix(oct, 8).unwrap_or(0)
        } else if let Some(bin) = text.strip_prefix("2#") {
            i64::from_str_radix(bin, 2).unwrap_or(0)
        } else {
            text.parse().unwrap_or(0)
        };
        Literal {
            kind: LiteralKind::Integer(value),
            range: self.range(node),
        }
    }

    fn lower_real_literal(&mut self, node: Node) -> Literal {
        let text = self.text(node).replace('_', "");
        let value = text.parse().unwrap_or(0.0);
        Literal {
            kind: LiteralKind::Real(value),
            range: self.range(node),
        }
    }

    fn lower_string_literal(&mut self, node: Node) -> Literal {
        let text = self.text(node);
        // Strip surrounding quotes
        let inner = if text.len() >= 2 {
            &text[1..text.len() - 1]
        } else {
            text
        };
        Literal {
            kind: LiteralKind::String(inner.to_string()),
            range: self.range(node),
        }
    }

    fn lower_boolean_literal(&mut self, node: Node) -> Literal {
        let value = self.text(node).to_uppercase() == "TRUE";
        Literal {
            kind: LiteralKind::Bool(value),
            range: self.range(node),
        }
    }

    fn lower_typed_literal(&mut self, node: Node) -> Literal {
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_elementary_type(self.text(n)))
            .unwrap_or(ElementaryType::Int);
        let raw_value = node
            .child_by_field_name("value")
            .map(|n| self.text(n).to_string())
            .unwrap_or_default();
        Literal {
            kind: LiteralKind::Typed { ty, raw_value },
            range: self.range(node),
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn lower_identifier(&self, node: Node) -> Identifier {
        Identifier {
            name: self.text(node).to_string(),
            range: self.range(node),
        }
    }

    fn field_identifier(&mut self, node: Node, field: &str) -> Identifier {
        node.child_by_field_name(field)
            .map(|n| self.lower_identifier(n))
            .unwrap_or_else(|| {
                self.error(format!("missing {field}"), node);
                self.dummy_identifier(node)
            })
    }

    fn lower_qualified_name(&self, node: Node) -> QualifiedName {
        let mut parts = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == kind::IDENTIFIER {
                parts.push(self.lower_identifier(child));
            }
        }
        QualifiedName {
            parts,
            range: self.range(node),
        }
    }

    fn dummy_identifier(&self, node: Node) -> Identifier {
        Identifier {
            name: "<missing>".to_string(),
            range: self.range(node),
        }
    }

    fn dummy_expression(&self, node: Node) -> Expression {
        Expression::Literal(Literal {
            kind: LiteralKind::Integer(0),
            range: self.range(node),
        })
    }

    fn dummy_variable_access(&self, node: Node) -> VariableAccess {
        VariableAccess {
            parts: vec![AccessPart::Identifier(self.dummy_identifier(node))],
            range: self.range(node),
        }
    }

    fn parse_var_kind(&self, text: &str) -> VarKind {
        match text.to_uppercase().as_str() {
            "VAR" => VarKind::Var,
            "VAR_INPUT" => VarKind::VarInput,
            "VAR_OUTPUT" => VarKind::VarOutput,
            "VAR_IN_OUT" => VarKind::VarInOut,
            "VAR_GLOBAL" => VarKind::VarGlobal,
            "VAR_EXTERNAL" => VarKind::VarExternal,
            "VAR_TEMP" => VarKind::VarTemp,
            _ => VarKind::Var,
        }
    }

    fn parse_var_qualifier(&self, text: &str) -> Option<VarQualifier> {
        match text.to_uppercase().as_str() {
            "RETAIN" => Some(VarQualifier::Retain),
            "PERSISTENT" => Some(VarQualifier::Persistent),
            "CONSTANT" => Some(VarQualifier::Constant),
            _ => None,
        }
    }

    fn try_parse_elementary_type(&self, text: &str) -> Option<ElementaryType> {
        match text.to_uppercase().as_str() {
            "BOOL" => Some(ElementaryType::Bool),
            "SINT" => Some(ElementaryType::Sint),
            "INT" => Some(ElementaryType::Int),
            "DINT" => Some(ElementaryType::Dint),
            "LINT" => Some(ElementaryType::Lint),
            "USINT" => Some(ElementaryType::Usint),
            "UINT" => Some(ElementaryType::Uint),
            "UDINT" => Some(ElementaryType::Udint),
            "ULINT" => Some(ElementaryType::Ulint),
            "REAL" => Some(ElementaryType::Real),
            "LREAL" => Some(ElementaryType::Lreal),
            "BYTE" => Some(ElementaryType::Byte),
            "WORD" => Some(ElementaryType::Word),
            "DWORD" => Some(ElementaryType::Dword),
            "LWORD" => Some(ElementaryType::Lword),
            "TIME" => Some(ElementaryType::Time),
            "LTIME" => Some(ElementaryType::Ltime),
            "DATE" => Some(ElementaryType::Date),
            "LDATE" => Some(ElementaryType::Ldate),
            "TIME_OF_DAY" | "TOD" => Some(ElementaryType::Tod),
            "LTOD" => Some(ElementaryType::Ltod),
            "DATE_AND_TIME" | "DT" => Some(ElementaryType::Dt),
            "LDT" => Some(ElementaryType::Ldt),
            _ => None,
        }
    }

    fn parse_elementary_type(&self, text: &str) -> ElementaryType {
        self.try_parse_elementary_type(text).unwrap_or(ElementaryType::Int)
    }
}
