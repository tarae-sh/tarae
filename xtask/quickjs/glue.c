// The code tool's engine (.sb/codemode-compute-plan.md §3; tarae's host is src/codetool.rs): QuickJS-ng as a
// WebAssembly module with no imports, driven by two exports. A program is the body of an async function; what it
// awaits from `tool(name, arg)` comes back as a list of pending calls, which the host answers one by one through
// `resume`.
//
//   alloc(len) -> ptr                    a buffer of len + 1 bytes the host writes into (text, NUL added here)
//   start(src, len, steps) -> reply      a fresh runtime and context; runs the program until it waits or ends,
//                                        `steps` its limit (below)
//   resume(id, answer, len, err) -> reply   answers call `id` with JSON text (rejects it when err != 0)
//
// A reply is a u32 length then that many bytes: a header line — `pending <id>:<name> …`, `done` or `error` —
// then JSON: {"pending":[{id,name,arg}]}, or {"done":value,…} / {"error":message,…}, each of the last two with
// "printed" (what print() wrote, its first PRINTED_MAX bytes), "printed_bytes" (all it was asked to write),
// "steps" (the program's so far), "syntax":true when the program didn't compile, and, when the step limit ended
// it, "stopped":"steps".
//
// A step is a call of QuickJS's interrupt handler: every 10,000 bytecode ops (JS_INTERRUPT_COUNTER_INIT), and
// every so many steps of the regex engine (lre_poll_timeout). The same count on every machine and under the JIT
// or the interpreter: the limit of one program is work, not seconds. Past it the handler interrupts, and goes
// on interrupting whatever JS runs after (a `toJSON` of the result too) — QuickJS's "interrupted" can't be
// caught.
//
// Included whole so the glue can reach the context's eval hook: once the program is compiled it's cleared, and
// `eval`, `Function`, and every constructor reached through a function's `.constructor` throw.
#include "quickjs.c"

#define MAX_CALLS 64
#define PRINTED_MAX (1 << 20)
#define MEMORY_LIMIT (48 << 20)
#define STACK_LIMIT (768 << 10)

static JSRuntime *rt;
static JSContext *cx;
static JSValue program;
static JSValue resolve[MAX_CALLS][2];
static int calls;
static JSValue waiting; // [{id, name, arg}] asked since the last reply
static uint32_t waiting_n;
static DynBuf printed;
static uint64_t printed_bytes;
static uint32_t steps, max_steps;
static int stopped, syntax;
static uint8_t *reply;

static int interrupt(JSRuntime *r, void *opaque) {
    if (!stopped && ++steps > max_steps)
        stopped = 1;
    return stopped;
}

__attribute__((export_name("alloc"))) char *alloc(uint32_t len) { return malloc((size_t)len + 1); }

// Kept up to PRINTED_MAX (whole pieces only: a character is never cut), every byte counted.
static void put_printed(const char *s, size_t len) {
    printed_bytes += len;
    if (printed.size + len <= PRINTED_MAX)
        dbuf_put(&printed, (const uint8_t *)s, len);
}

static void print_value(JSContext *ctx, JSValueConst v) {
    size_t len;
    const char *s = JS_ToCStringLen(ctx, &len, v);
    if (!s) {
        JS_FreeValue(ctx, JS_GetException(ctx));
        s = JS_ToCStringLen(ctx, &len, JS_NewString(ctx, "[unprintable]"));
    }
    put_printed(s, len);
    JS_FreeCString(ctx, s);
}

static JSValue js_print(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    for (int i = 0; i < argc; i++) {
        if (i)
            put_printed(" ", 1);
        print_value(ctx, argv[i]);
    }
    put_printed("\n", 1);
    return JS_UNDEFINED;
}

// The one stub binding: tool(name, arg) -> a promise the host settles.
static JSValue js_tool(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    if (calls >= MAX_CALLS)
        return JS_ThrowRangeError(ctx, "%d calls is the limit of one program", MAX_CALLS);
    const char *name = argc > 0 ? JS_ToCString(ctx, argv[0]) : NULL;
    if (!name)
        return JS_ThrowTypeError(ctx, "tool(name, arg): name is a string");
    int id = calls++;
    JSValue p = JS_NewPromiseCapability(ctx, resolve[id]);
    JSValue c = JS_NewObject(ctx);
    JS_SetPropertyStr(ctx, c, "id", JS_NewInt32(ctx, id));
    JS_SetPropertyStr(ctx, c, "name", JS_NewString(ctx, name));
    JS_SetPropertyStr(ctx, c, "arg", argc > 1 ? JS_DupValue(ctx, argv[1]) : JS_UNDEFINED);
    JS_FreeCString(ctx, name);
    JS_SetPropertyUint32(ctx, waiting, waiting_n++, c);
    return p;
}

// The end's counts, on a reply's body.
static void counts(JSValue body) {
    JS_SetPropertyStr(cx, body, "printed", JS_NewStringLen(cx, (const char *)printed.buf, printed.size));
    JS_SetPropertyStr(cx, body, "printed_bytes", JS_NewFloat64(cx, (double)printed_bytes));
    JS_SetPropertyStr(cx, body, "steps", JS_NewUint32(cx, steps));
    if (stopped)
        JS_SetPropertyStr(cx, body, "stopped", JS_NewString(cx, "steps"));
    if (syntax)
        JS_SetPropertyStr(cx, body, "syntax", JS_TRUE);
}

static uint8_t *answer(const char *head, JSValue body) {
    JSValue text = JS_JSONStringify(cx, body, JS_UNDEFINED, JS_UNDEFINED);
    if (JS_IsException(text) || !JS_IsString(text)) {
        // the result's own toJSON threw, or ran past the limit, or it can't be said in JSON (a cycle, a BigInt)
        JS_FreeValue(cx, JS_IsException(text) ? JS_GetException(cx) : text);
        char e[160];
        snprintf(e, sizeof e, "{\"error\":\"the result can't be turned into JSON\",\"steps\":%u%s}", steps,
                 stopped ? ",\"stopped\":\"steps\"" : "");
        head = "error";
        text = JS_NewString(cx, e);
    }
    size_t len, hlen = strlen(head);
    const char *s = JS_ToCStringLen(cx, &len, text);
    free(reply);
    reply = malloc(4 + hlen + 1 + len);
    uint32_t total = hlen + 1 + len;
    memcpy(reply, &total, 4);
    memcpy(reply + 4, head, hlen);
    reply[4 + hlen] = '\n';
    memcpy(reply + 4 + hlen + 1, s, len);
    JS_FreeCString(cx, s);
    JS_FreeValue(cx, text);
    JS_FreeValue(cx, body);
    return reply;
}

// The exception's words, and its stack when it has one.
static uint8_t *failed(JSValue exc) {
    DynBuf b;
    dbuf_init(&b);
    const char *s = JS_ToCString(cx, exc);
    if (!s)
        JS_FreeValue(cx, JS_GetException(cx));
    dbuf_putstr(&b, s ? s : "[an exception that can't be said]");
    JS_FreeCString(cx, s);
    if (JS_IsObject(exc)) {
        JSValue stack = JS_GetPropertyStr(cx, exc, "stack");
        const char *t = JS_IsString(stack) ? JS_ToCString(cx, stack) : NULL;
        if (t && *t) {
            dbuf_putc(&b, '\n');
            dbuf_putstr(&b, t);
        }
        JS_FreeCString(cx, t);
        JS_FreeValue(cx, stack);
    }
    JS_FreeValue(cx, exc);
    JSValue body = JS_NewObject(cx);
    JS_SetPropertyStr(cx, body, "error", JS_NewStringLen(cx, (const char *)b.buf, b.size));
    dbuf_free(&b);
    counts(body);
    return answer("error", body);
}

// Runs every job; then the program has ended, or waits on what it asked, or waits on nothing (an error).
static uint8_t *settle(void) {
    for (;;) {
        JSContext *c;
        int r = JS_ExecutePendingJob(rt, &c);
        if (r < 0)
            return failed(JS_GetException(cx));
        if (r == 0)
            break;
    }
    switch (JS_PromiseState(cx, program)) {
    case JS_PROMISE_FULFILLED: {
        JSValue body = JS_NewObject(cx);
        JS_SetPropertyStr(cx, body, "done", JS_PromiseResult(cx, program));
        counts(body);
        return answer("done", body);
    }
    case JS_PROMISE_REJECTED:
        return failed(JS_PromiseResult(cx, program));
    default:
        break;
    }
    uint32_t n = waiting_n;
    if (n == 0) {
        // still out: calls asked in an earlier reply, not answered yet (the rest of a Promise.all)
        for (int i = 0; i < calls; i++)
            if (!JS_IsUndefined(resolve[i][0])) {
                JSValue body = JS_NewObject(cx);
                JS_SetPropertyStr(cx, body, "pending", JS_NewArray(cx));
                return answer("pending", body);
            }
        return failed(JS_NewString(cx, "the program waits on a promise nothing will settle"));
    }
    char head[16 + MAX_CALLS * 64] = "pending";
    size_t at = strlen(head);
    for (uint32_t i = 0; i < n; i++) {
        JSValue c = JS_GetPropertyUint32(cx, waiting, i);
        JSValue id = JS_GetPropertyStr(cx, c, "id");
        int32_t idn;
        JS_ToInt32(cx, &idn, id);
        JSValue name = JS_GetPropertyStr(cx, c, "name");
        const char *s = JS_ToCString(cx, name);
        at += snprintf(head + at, sizeof head - at, " %d:%.48s", idn, s);
        JS_FreeCString(cx, s);
        JS_FreeValue(cx, name);
        JS_FreeValue(cx, c);
    }
    JSValue body = JS_NewObject(cx);
    JS_SetPropertyStr(cx, body, "pending", waiting);
    waiting = JS_NewArray(cx);
    waiting_n = 0;
    return answer(head, body);
}

static void fresh(void) {
    if (rt) {
        for (int i = 0; i < calls; i++) {
            JS_FreeValue(cx, resolve[i][0]);
            JS_FreeValue(cx, resolve[i][1]);
        }
        JS_FreeValue(cx, program);
        JS_FreeValue(cx, waiting);
        JS_FreeContext(cx);
        JS_FreeRuntime(rt);
        dbuf_free(&printed);
    }
    calls = 0;
    waiting_n = 0;
    printed_bytes = 0;
    steps = 0;
    stopped = 0;
    syntax = 0;
    rt = JS_NewRuntime();
    JS_SetMemoryLimit(rt, MEMORY_LIMIT);
    JS_SetInterruptHandler(rt, interrupt, NULL);
    // QuickJS-ng turns the stack check off on wasi (`update_stack_limit`); put it back, within the module's 1 MB
    // shadow stack (linked first: past it is below address 0, a trap), so deep recursion is a RangeError
    rt->stack_size = STACK_LIMIT;
    rt->stack_top = js_get_stack_pointer();
    rt->stack_limit = rt->stack_top - STACK_LIMIT;
    cx = JS_NewContext(rt);
    dbuf_init(&printed);
    waiting = JS_NewArray(cx);
    program = JS_UNDEFINED;
    JSValue g = JS_GetGlobalObject(cx);
    JS_SetPropertyStr(cx, g, "print", JS_NewCFunction(cx, js_print, "print", 1));
    JS_SetPropertyStr(cx, g, "tool", JS_NewCFunction(cx, js_tool, "tool", 2));
    JSAtom eval = JS_NewAtom(cx, "eval");
    JS_DeleteProperty(cx, g, eval, 0);
    JS_FreeAtom(cx, eval);
    JS_FreeValue(cx, g);
}

__attribute__((export_name("start"))) uint8_t *start(char *src, uint32_t len, uint32_t limit) {
    src[len] = 0;
    fresh();
    max_steps = limit;
    // (on the program's first line, so an error's line is the program's own)
    static const char before[] = "(async () => {", after[] = "\n})()";
    DynBuf b;
    dbuf_init(&b);
    dbuf_put(&b, (const uint8_t *)before, sizeof before - 1);
    dbuf_put(&b, (const uint8_t *)src, len);
    dbuf_put(&b, (const uint8_t *)after, sizeof after);
    free(src);
    JSValue f = JS_Eval(cx, (const char *)b.buf, b.size - 1, "program.js", JS_EVAL_TYPE_GLOBAL | JS_EVAL_FLAG_COMPILE_ONLY);
    dbuf_free(&b);
    // from here on nothing becomes code: eval and every Function constructor throw
    cx->eval_internal = NULL;
    if (JS_IsException(f)) {
        syntax = 1;
        return failed(JS_GetException(cx));
    }
    program = JS_EvalFunction(cx, f);
    if (JS_IsException(program)) {
        program = JS_UNDEFINED;
        return failed(JS_GetException(cx));
    }
    return settle();
}

__attribute__((export_name("resume"))) uint8_t *resume(uint32_t id, char *text, uint32_t len, uint32_t err) {
    text[len] = 0;
    if (id >= (uint32_t)calls || JS_IsUndefined(resolve[id][0])) {
        free(text);
        return failed(JS_NewString(cx, "resume: no such call"));
    }
    JSValue v = JS_ParseJSON(cx, text, len, "answer");
    free(text);
    if (JS_IsException(v))
        v = JS_GetException(cx);
    JSValue r = JS_Call(cx, resolve[id][err ? 1 : 0], JS_UNDEFINED, 1, (JSValueConst *)&v);
    JS_FreeValue(cx, r);
    JS_FreeValue(cx, v);
    JS_FreeValue(cx, resolve[id][0]);
    JS_FreeValue(cx, resolve[id][1]);
    resolve[id][0] = resolve[id][1] = JS_UNDEFINED;
    return settle();
}
