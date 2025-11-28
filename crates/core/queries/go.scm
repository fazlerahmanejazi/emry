; Go tags query file

; Function declarations
(function_declaration
  name: (identifier) @definition.function)

; Method declarations
(method_declaration
  name: (field_identifier) @definition.method)

; Type declarations (structs)
(type_declaration
  (type_spec
    name: (type_identifier) @definition.class
    type: (struct_type)))

; Type declarations (interfaces)
(type_declaration
  (type_spec
    name: (type_identifier) @definition.interface
    type: (interface_type)))
