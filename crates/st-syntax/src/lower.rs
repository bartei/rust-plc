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

/// Map tree-sitter node kind strings to user-friendly names.
fn friendly_kind_name(kind: &str) -> &str {
    match kind {
        "identifier" => "identifier",
        ";" | "MISSING ;" => "';'",
        "END_IF" => "END_IF",
        "END_FOR" => "END_FOR",
        "END_WHILE" => "END_WHILE",
        "END_CASE" => "END_CASE",
        "END_REPEAT" => "END_REPEAT",
        "END_VAR" => "END_VAR",
        "END_PROGRAM" => "END_PROGRAM",
        "END_FUNCTION" => "END_FUNCTION",
        "END_FUNCTION_BLOCK" => "END_FUNCTION_BLOCK",
        "END_CLASS" => "END_CLASS",
        "THEN" => "THEN after IF condition",
        "DO" => "DO after FOR/WHILE condition",
        "OF" => "OF after CASE expression",
        "statement_list" => "statement",
        "expression" | "additive_expression" | "comparison_expression" => "expression",
        "variable_declaration" => "variable declaration",
        "assignment_statement" => "assignment",
        _ => kind,
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
    ///
    /// When tree-sitter encounters a parse error, it creates an ERROR node
    /// that can span a large region (from the error to the next recovery
    /// point). We report errors at the **start** of the ERROR node and
    /// extract context from the node's children and parent to produce a
    /// useful message like "unexpected token ';'" or "expected END_IF".
    fn collect_cst_errors(&mut self, node: Node) {
        if node.is_error() {
            let msg = self.describe_error(node);
            // Report at the START of the error, not the entire span — the
            // start is where the actual problem is.
            let start = node.start_byte();
            let end = std::cmp::min(
                node.end_byte(),
                // Limit the squiggle to one line for readability.
                self.source.get(start..)
                    .and_then(|s| s.iter().position(|&b| b == b'\n'))
                    .map(|nl| start + nl)
                    .unwrap_or(node.end_byte()),
            );
            self.errors.push(LowerError {
                message: msg,
                range: TextRange::new(start, end),
            });
            return; // Don't recurse into ERROR node children — one report is enough.
        }
        if node.is_missing() {
            let kind = node.kind();
            // Generate a friendly name for the missing element.
            let friendly = friendly_kind_name(kind);
            self.error(format!("expected {friendly}"), node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.has_error() || child.is_error() || child.is_missing() {
                self.collect_cst_errors(child);
            }
        }
    }

    /// Produce a helpful error message by examining the ERROR node's content
    /// and its surrounding context.
    fn describe_error(&self, error_node: Node) -> String {
        // Check what text the ERROR node contains — this is the unexpected input.
        let error_text = self.text(error_node).trim();

        // Look at the first non-error child to find the unexpected token.
        let first_child = {
            let mut cursor = error_node.walk();
            error_node.children(&mut cursor)
                .find(|c| !c.is_extra())
                .map(|c| self.text(c).trim().to_string())
        };

        // Look at the parent to understand what was expected.
        let parent = error_node.parent();
        let parent_kind = parent.map(|p| p.kind()).unwrap_or("");

        // Check what comes after the error for missing-keyword hints.
        let next_sibling = error_node.next_named_sibling();

        // Generate a contextual message.
        match parent_kind {
            "if_statement" => {
                if error_text.contains("IF") || error_text.contains("if") {
                    return "expected END_IF to close IF block".to_string();
                }
                return "syntax error in IF statement — check THEN/END_IF".to_string();
            }
            "for_statement" => {
                return "syntax error in FOR statement — check DO/END_FOR".to_string();
            }
            "while_statement" => {
                return "syntax error in WHILE statement — check DO/END_WHILE".to_string();
            }
            "case_statement" => {
                return "syntax error in CASE statement — check END_CASE".to_string();
            }
            "program_declaration" => {
                if next_sibling.is_none() {
                    return "expected END_PROGRAM".to_string();
                }
            }
            "function_declaration" => {
                if next_sibling.is_none() {
                    return "expected END_FUNCTION".to_string();
                }
            }
            "function_block_declaration" => {
                if next_sibling.is_none() {
                    return "expected END_FUNCTION_BLOCK".to_string();
                }
            }
            "class_declaration" => {
                if next_sibling.is_none() {
                    return "expected END_CLASS".to_string();
                }
            }
            "var_block" => {
                return "syntax error in variable declaration — check END_VAR".to_string();
            }
            _ => {}
        }

        // Use the first child or error text to describe the unexpected token.
        let token = first_child
            .as_deref()
            .unwrap_or(error_text);
        let token_short = if token.len() > 30 {
            &token[..30]
        } else {
            token
        };
        if !token_short.is_empty() {
            format!("unexpected '{token_short}'")
        } else {
            "syntax error".to_string()
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
                kind::CLASS_DECLARATION => {
                    items.push(TopLevelItem::Class(self.lower_class(child)));
                }
                kind::INTERFACE_DECLARATION => {
                    items.push(TopLevelItem::Interface(self.lower_interface(child)));
                }
                kind::GLOBAL_VAR_DECLARATION => {
                    items.push(TopLevelItem::GlobalVarDeclaration(
                        self.lower_global_var_block(child),
                    ));
                }
                _ if child.is_error() => {
                    let msg = self.describe_error(child);
                    self.errors.push(LowerError {
                        message: msg,
                        range: TextRange::new(child.start_byte(), child.start_byte() + 1),
                    });
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
    // OOP extensions
    // =========================================================================

    fn lower_class(&mut self, node: Node) -> ClassDecl {
        let name = self.field_identifier(node, "name");
        let is_abstract = node.child_by_field_name("abstract").is_some();
        let is_final = node.child_by_field_name("final").is_some();
        let base_class = node
            .child_by_field_name("base")
            .map(|n| self.text(n).to_string());

        // Collect IMPLEMENTS interface list via field name
        let mut interfaces = Vec::new();
        let mut cursor_iface = node.walk();
        for child in node.children_by_field_name("interfaces", &mut cursor_iface) {
            if child.kind() == kind::IDENTIFIER {
                interfaces.push(self.text(child).to_string());
            }
        }

        let var_blocks = self.collect_var_blocks(node);

        let mut methods = Vec::new();
        let mut properties = Vec::new();
        let mut cursor2 = node.walk();
        for child in node.children(&mut cursor2) {
            match child.kind() {
                kind::METHOD_DECLARATION => methods.push(self.lower_method(child)),
                kind::PROPERTY_DECLARATION => properties.push(self.lower_property(child)),
                _ => {}
            }
        }

        ClassDecl {
            name,
            base_class,
            interfaces,
            is_abstract,
            is_final,
            var_blocks,
            methods,
            properties,
            range: self.range(node),
        }
    }

    fn lower_method(&mut self, node: Node) -> MethodDecl {
        let access = node
            .child_by_field_name("access")
            .map(|n| self.parse_access_specifier(self.text(n)))
            .unwrap_or(AccessSpecifier::Public);
        let is_abstract = node.child_by_field_name("abstract").is_some();
        let is_final = node.child_by_field_name("final").is_some();
        let is_override = node.child_by_field_name("override").is_some();
        let name = self.field_identifier(node, "name");
        let return_type = node
            .child_by_field_name("return_type")
            .map(|n| self.lower_data_type(n));
        let var_blocks = self.collect_var_blocks(node);
        let body = self.field_statements(node, "body");

        MethodDecl {
            access,
            name,
            return_type,
            var_blocks,
            body,
            is_abstract,
            is_final,
            is_override,
            range: self.range(node),
        }
    }

    fn lower_interface(&mut self, node: Node) -> InterfaceDecl {
        let name = self.field_identifier(node, "name");

        // Collect EXTENDS base interface names via field name
        let mut base_interfaces = Vec::new();
        let mut cursor_base = node.walk();
        for child in node.children_by_field_name("base", &mut cursor_base) {
            if child.kind() == kind::IDENTIFIER {
                base_interfaces.push(self.text(child).to_string());
            }
        }

        let mut methods = Vec::new();
        let mut cursor2 = node.walk();
        for child in node.children(&mut cursor2) {
            if child.kind() == kind::METHOD_PROTOTYPE {
                methods.push(self.lower_method_prototype(child));
            }
        }

        InterfaceDecl {
            name,
            base_interfaces,
            methods,
            range: self.range(node),
        }
    }

    fn lower_method_prototype(&mut self, node: Node) -> MethodPrototype {
        let name = self.field_identifier(node, "name");
        let return_type = node
            .child_by_field_name("return_type")
            .map(|n| self.lower_data_type(n));
        let var_blocks = self.collect_var_blocks(node);

        MethodPrototype {
            name,
            return_type,
            var_blocks,
            range: self.range(node),
        }
    }

    fn lower_property(&mut self, node: Node) -> PropertyDecl {
        let access = node
            .child_by_field_name("access")
            .map(|n| self.parse_access_specifier(self.text(n)))
            .unwrap_or(AccessSpecifier::Public);
        let name = self.field_identifier(node, "name");
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_data_type(n))
            .unwrap_or(DataType::Elementary(ElementaryType::Int));

        let get_body = node
            .child_by_field_name("get")
            .map(|n| self.lower_property_accessor(n));
        let set_body = node
            .child_by_field_name("set")
            .map(|n| self.lower_property_accessor(n));

        PropertyDecl {
            access,
            name,
            ty,
            get_body,
            set_body,
            range: self.range(node),
        }
    }

    fn lower_property_accessor(&mut self, node: Node) -> PropertyAccessor {
        let var_blocks = self.collect_var_blocks(node);
        let body = self.field_statements(node, "body");
        PropertyAccessor {
            var_blocks,
            body,
            range: self.range(node),
        }
    }

    fn parse_access_specifier(&self, text: &str) -> AccessSpecifier {
        match text.to_uppercase().as_str() {
            "PUBLIC" => AccessSpecifier::Public,
            "PRIVATE" => AccessSpecifier::Private,
            "PROTECTED" => AccessSpecifier::Protected,
            "INTERNAL" => AccessSpecifier::Internal,
            _ => AccessSpecifier::Public,
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
                let msg = self.describe_error(node);
                self.errors.push(LowerError {
                    message: msg,
                    range: TextRange::new(node.start_byte(), std::cmp::min(
                        node.end_byte(),
                        self.source.get(node.start_byte()..)
                            .and_then(|s| s.iter().position(|&b| b == b'\n'))
                            .map(|nl| node.start_byte() + nl)
                            .unwrap_or(node.end_byte()),
                    )),
                });
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
            kind::THIS_EXPRESSION => Expression::This(self.range(node)),
            kind::SUPER_EXPRESSION => Expression::Super(self.range(node)),
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
                kind::PARTIAL_ACCESS => {
                    if let Some(pa) = self.parse_partial_access(self.text(child)) {
                        parts.push(pa);
                    }
                }
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
                } else if child.kind() == kind::PARTIAL_ACCESS {
                    if let Some(pa) = self.parse_partial_access(self.text(child)) {
                        parts.push(pa);
                    }
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

    /// Parse a partial access token like ".%X0", ".%B1", ".%W0", ".%D0".
    fn parse_partial_access(&self, text: &str) -> Option<AccessPart> {
        // Format: .%<kind><index>
        let s = text.strip_prefix(".%").or_else(|| text.strip_prefix("."))?;
        if s.is_empty() {
            return None;
        }
        let kind_char = s.as_bytes()[0].to_ascii_uppercase();
        let index_str = &s[1..];
        let index: u32 = index_str.parse().ok()?;
        let kind = match kind_char {
            b'X' => PartialAccessKind::Bit,
            b'B' => PartialAccessKind::Byte,
            b'W' => PartialAccessKind::Word,
            b'D' => PartialAccessKind::DWord,
            b'L' => PartialAccessKind::LWord,
            _ => return None,
        };
        Some(AccessPart::Partial(kind, index))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_errors(source: &str) -> Vec<(usize, String)> {
        let result = crate::parse(source);
        result.errors.iter().map(|e| {
            let line = source[..e.range.start].lines().count();
            (line, e.message.clone())
        }).collect()
    }

    #[test]
    fn error_equals_instead_of_assign_points_to_correct_line() {
        // Common mistake: using = instead of := for assignment
        let source = "\
PROGRAM Main\n\
VAR\n\
    filling : BOOL := FALSE;\n\
    moving : BOOL := FALSE;\n\
END_VAR\n\
    IF TRUE THEN\n\
        filling = TRUE;\n\
        moving = FALSE;\n\
    END_IF;\n\
END_PROGRAM\n";
        let errors = parse_errors(source);
        eprintln!("= vs := errors: {errors:?}");
        assert!(!errors.is_empty(), "Should have errors for = instead of :=");
        // The errors should be on lines 7-8 (the bad assignments), NOT at the end
        let has_early_error = errors.iter().any(|e| e.0 >= 7 && e.0 <= 8);
        assert!(
            has_early_error,
            "Errors should point to lines 7-8 (the = assignments), got: {errors:?}"
        );
    }

    #[test]
    fn error_missing_end_if_points_to_if_line() {
        let errors = parse_errors(
            "PROGRAM Main\nVAR x : INT; END_VAR\n    IF x > 0 THEN\n        x := 1;\n    \n    x := 2;\nEND_PROGRAM\n"
        );
        eprintln!("Errors: {errors:?}");
        assert!(!errors.is_empty(), "Should have errors for missing END_IF");
        // The error should NOT be at the end of the file — it should be near
        // the IF statement or the point where the parser lost sync.
        let last_line = 7; // END_PROGRAM is line 7
        assert!(
            errors.iter().any(|e| e.0 < last_line),
            "At least one error should point before the last line, got: {errors:?}"
        );
    }

    #[test]
    fn error_missing_expression_gives_context() {
        let errors = parse_errors(
            "PROGRAM Main\nVAR x : INT; END_VAR\n    x := ;\nEND_PROGRAM\n"
        );
        eprintln!("Errors: {errors:?}");
        assert!(!errors.is_empty());
        // Error should be on the line with "x := ;" (line 3), not at the end.
        assert!(
            errors.iter().any(|e| e.0 == 3),
            "Error should be on line 3 (the assignment), got: {errors:?}"
        );
    }

    #[test]
    fn error_messages_not_all_generic() {
        let errors = parse_errors(
            "PROGRAM Main\nVAR x : INT; END_VAR\n    IF TRUE THEN\n        x := 1;\nEND_PROGRAM\n"
        );
        eprintln!("Errors: {errors:?}");
        // At least one error should have a more helpful message than just "syntax error"
        let has_context = errors.iter().any(|e| {
            e.1.contains("END_IF")
                || e.1.contains("unexpected")
                || e.1.contains("expected")
                || e.1.contains("IF statement")
        });
        assert!(
            has_context,
            "At least one error should mention END_IF or give context, got: {errors:?}"
        );
    }
}
