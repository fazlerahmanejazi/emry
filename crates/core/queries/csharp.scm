; C# tags query file

; Class declarations
(class_declaration
  name: (identifier) @definition.class)

; Interface declarations
(interface_declaration
  name: (identifier) @definition.interface)

; Struct declarations
(struct_declaration
  name: (identifier) @definition.class)

; Enum declarations
(enum_declaration
  name: (identifier) @definition.enum)

; Method declarations
(method_declaration
  name: (identifier) @definition.method)

; Namespace declarations
(namespace_declaration
  name: (identifier) @definition.module)
