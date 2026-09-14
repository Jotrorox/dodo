(function_declaration) @function.around
(function_declaration body: (block "{" (_)* @function.inside "}"))
(struct_declaration) @class.around
(struct_declaration body: (block "{" (_)* @class.inside "}"))
(enum_declaration) @class.around
(enum_declaration body: (block "{" (_)* @class.inside "}"))
(comment)+ @comment.around
