; Based on official tree-sitter-rust tags.scm
; Simplified for tree-sitter-tags 0.23 compatibility

; Struct definitions - just capture type_identifier
(struct_item
    (type_identifier) @definition.class)

; Enum definitions
(enum_item
    (type_identifier) @definition.class)

; Trait definitions
(trait_item
    (type_identifier) @definition.interface)

; Function definitions
(function_item
    (identifier) @definition.function)

; Constant definitions
(const_item
    (identifier) @definition.constant)
