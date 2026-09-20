/* qjs_knc.h: registration for the QuickJS `knc` module. See qjs_knc.md. */
#ifndef QJS_KNC_H
#define QJS_KNC_H

#include "quickjs.h"

JSModuleDef *js_init_module_knc(JSContext *ctx, const char *module_name);

#endif /* QJS_KNC_H */
