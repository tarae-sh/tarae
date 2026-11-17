// The code tool's engine (.sb/codemode-compute-plan.md §3; tarae's host is src/codetool.rs): the original QuickJS
// (Fabrice Bellard's, `pin`'s TAG) as a WebAssembly module with no imports, driven by two exports. A program is the
// body of an async function; what it awaits from `tool(name, arg)` comes back as a list of pending calls, which the
// host answers one by one through `resume`.
//
//   alloc(len) -> ptr                    a buffer of len + 1 bytes the host writes into (text, NUL added here)
//   start(src, len, steps) -> reply      a fresh runtime and context; runs the program until it waits or ends,
//                                        `steps` its limit (below)
//   resume(id, answer, len, err) -> reply   answers call `id` with JSON text (rejects it when err != 0)
//   oom() -> 0 | 1                       whether an allocation was refused (`oom` below), read after a trap only
//
// A reply is a u32 length then that many bytes: a header line — `pending <id>:<name> …`, `done` or `error` —
// then JSON: {"pending":[{id,name,arg}]}, or {"done":value,…} / {"error":message,…}, each of the last two with
// "printed" (what print() wrote, its first PRINTED_MAX bytes), "printed_bytes" (all it was asked to write),
// "steps" (the program's so far), "syntax":true when the program didn't compile, and, when the step limit ended
// it, "stopped":"steps".
//
// Out of memory: the cap is the engine's own allocator refusing past MEMORY_LIMIT (`JS_SetMemoryLimit`), with or
// without what follows. The original throws an `out of memory` a program's `catch` can take; the glue makes the
// ending final and exact: from the first refused allocation the interrupt handler stops the program at its very next
// poll, and whatever it replies — a value, a wait or an error — is OUT_OF_MEMORY, built in the glue's own memory.
// `codetool::tests::the_engine_never_retries_a_refused_allocation` walks every refusal of its bodies on the probe
// module (GLUE_PROBE below): without the guard the engine never carries on past one but where it gives up an
// optional growth (a Map's hash table), so the ending is never early; with it, every one ends the program, though
// the engine alone swallows some (that Map, a job it couldn't queue). A refusal while the program compiles can end in
// a trap instead (the compiler reads on past a bytecode buffer it couldn't grow): the host reads `oom()` after the
// trap and says the memory limit, as it does when the module's own memory (`--max-memory`) refused first.
//
// A step is a call of QuickJS's interrupt handler: every 10,000 of a program's loop turns and branches (the
// handler's counter, JS_INTERRUPT_COUNTER_INIT, is spent at every jump), and every so many steps of the regex
// engine (lre_poll_timeout). The same count on every machine and under the JIT
// or the interpreter: the limit of one program is work, not seconds. Past it the handler interrupts, and goes
// on interrupting whatever JS runs after (a `toJSON` of the result too) — QuickJS's "interrupted" can't be
// caught.
//
// Included whole so the glue can reach the context's eval hook: once the program is compiled it's cleared, and
// `eval`, `Function`, and every constructor reached through a function's `.constructor` throw.
//
// The original's heap accounting (`js_def_malloc_usable_size`) calls malloc_usable_size, which it declares for glibc
// alone; wasi-libc's dlmalloc has it in <malloc.h>. Without it the source's fallback is `return 0`, and the heap
// limit would count allocations' overheads only.
#include <malloc.h>
#include "quickjs.c"
// The original forbids libc's allocator past its own (`#define malloc malloc_is_forbidden`); the glue's host buffers
// (`alloc`, the reply) are libc's on purpose, outside the engine's heap limit.
#undef malloc
#undef free
#undef realloc

#define MAX_CALLS 64
#define PRINTED_MAX (1 << 20)
#define MEMORY_LIMIT (48 << 20)
#define STACK_LIMIT (768 << 10)
#define OUT_OF_MEMORY "InternalError: out of memory"

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
static uint8_t *reply;            // libc's, or `spare`
static uint8_t spare[256];        // the out-of-memory reply when libc has no room either

// Set by the first refused allocation of the program (`refused`), cleared by `fresh`.
static int oom;

// The flag, for the host: read after a trap only, it chooses the trap's words — a refusal noted, the memory limit's
// (a refusal while compiling can trap inside the engine: the compiler reads on past a buffer it couldn't grow).
__attribute__((export_name("oom"))) uint32_t oom_noted(void) { return oom; }

#ifdef GLUE_PROBE
// The tests' probe module (never shipped; inside.sh links it apart, and a test holds quickjs.wasm to having none of
// these exports): `refuse_at(n, guard)` refuses the n-th growing allocation of the next `start` (0: none), with the
// guard's ending on or off — off, what the engine itself makes of a refusal is what the program sees; `grows()` says
// how many there were, `compile_grows()` how many of them the compile's. A shrinking realloc is never refused
// (`js_def_realloc` can't refuse one on the limit either).
static uint32_t refuse_n, grown, compiled_n;
static int counting, guard_on; // counting: from the compile to the end of the jobs — the glue's own reply isn't the engine's
__attribute__((export_name("refuse_at"))) void refuse_at(uint32_t n, uint32_t guard) {
    refuse_n = n;
    guard_on = guard != 0;
}
__attribute__((export_name("grows"))) uint32_t grows(void) { return grown; }
__attribute__((export_name("compile_grows"))) uint32_t compile_grows(void) { return compiled_n; }
static int refuse_now(void) { return counting && ++grown == refuse_n; }
#endif

// A refusal: the program ends at the very next poll, not up to JS_INTERRUPT_COUNTER_INIT branches later (a builtin
// that fails throws at once — the engine never retries — and the next poll comes at a backward jump, a JS call's entry
// or inside some builtins' loops). Seen here, under the engine, it's seen where the engine swallows one too (a Map's
// hash table not grown, a job not queued).
static void refused(void) {
#ifdef GLUE_PROBE
    if (!guard_on)
        return;
#endif
    oom = 1;
    if (cx)
        cx->interrupt_counter = 1;
}

// The original's allocator (`js_def_malloc`, `js_def_realloc`: the heap limit is theirs), every refusal seen: they
// sit under its 4 KB arenas and large blocks, which call them for every page and block they take.
static void *glue_malloc(JSMallocState *s, size_t n) {
#ifdef GLUE_PROBE
    if (refuse_now()) {
        refused();
        return NULL;
    }
#endif
    void *p = js_def_malloc(s, n);
    if (!p)
        refused();
    return p;
}
static void *glue_realloc(JSMallocState *s, void *p, size_t n) {
#ifdef GLUE_PROBE
    if (n && (!p || n > js_def_malloc_usable_size(p)) && refuse_now()) {
        refused();
        return NULL;
    }
#endif
    void *q = js_def_realloc(s, p, n);
    if (!q && n)
        refused();
    return q;
}
static const JSMallocFunctions glue_heap = {glue_malloc, js_def_free, glue_realloc, js_def_malloc_usable_size};

// The original's JSInterruptHandler takes the runtime. Past a refusal it interrupts whatever runs, steps uncounted.
static int interrupt(JSRuntime *r, void *opaque) {
    if (oom)
        return 1;
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

// The reply's room, `n` bytes from libc (the last one's freed): NULL when there's none.
static uint8_t *room(size_t n) {
    if (reply != spare)
        free(reply);
    reply = malloc(n);
    return reply;
}

// A string's JSON escapes into `to` (NULL: only counted), the bytes they take. Bytes past ASCII go as they are: the
// host reads the reply's text lossily.
static size_t json_escaped(uint8_t *to, const uint8_t *s, size_t n) {
    static const char hex[] = "0123456789abcdef";
    size_t k = 0;
    for (size_t i = 0; i < n; i++) {
        uint8_t c = s[i];
        if (c == '"' || c == '\\') {
            if (to)
                to[k] = '\\', to[k + 1] = c;
            k += 2;
        } else if (c < 0x20) {
            if (to)
                memcpy(to + k, (const char[]){'\\', 'u', '0', '0', hex[c >> 4], hex[c & 15]}, 6);
            k += 6;
        } else {
            if (to)
                to[k] = c;
            k++;
        }
    }
    return k;
}

// The reply after a refusal, made without the engine: its heap is at its limit, so nothing is asked of it. What was
// printed is kept when libc has room for it; else the words alone, in `spare`.
static uint8_t *out_of_memory(void) {
    static const char lead[] = "error\n{\"error\":\"" OUT_OF_MEMORY "\",\"printed\":\"";
    char tail[96];
    size_t tl = snprintf(tail, sizeof tail, "\",\"printed_bytes\":%llu,\"steps\":%u%s}",
                         (unsigned long long)printed_bytes, steps, stopped ? ",\"stopped\":\"steps\"" : "");
    size_t pl = json_escaped(NULL, printed.buf, printed.size);
    if (!room(4 + sizeof lead - 1 + pl + tl)) {
        reply = spare;
        pl = 0;
    }
    uint32_t total = sizeof lead - 1 + pl + tl;
    memcpy(reply, &total, 4);
    memcpy(reply + 4, lead, sizeof lead - 1);
    if (pl)
        json_escaped(reply + 4 + sizeof lead - 1, printed.buf, printed.size);
    memcpy(reply + 4 + sizeof lead - 1 + pl, tail, tl);
    return reply;
}

static uint8_t *answer(const char *head, JSValue body) {
    JSValue text = JS_JSONStringify(cx, body, JS_UNDEFINED, JS_UNDEFINED);
    if (JS_IsException(text) && oom) {
        // refused while the reply was put into words: that refusal is the ending
        JS_FreeValue(cx, JS_GetException(cx));
        JS_FreeValue(cx, body);
        return out_of_memory();
    }
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
    room(4 + hlen + 1 + len);
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

// The exception's words, and its stack when it has one; past a refusal, out of memory whatever was thrown.
static uint8_t *failed(JSValue exc) {
#ifdef GLUE_PROBE
    counting = 0;
#endif
    if (oom) {
        JS_FreeValue(cx, exc);
        return out_of_memory();
    }
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
#ifdef GLUE_PROBE
    counting = 0;
#endif
    // (No check of `oom` here: past a refusal, a value or a wait can't get this far. The counter's nudge makes the
    // next poll interrupt, and even a `return` polls, at the entry of the call that resolves the program's promise,
    // so the ending comes through `failed`; a refusal inside `tool()` breaks the call it records, and `answer` says
    // it. The day a path does get here, test d's `return 'ok'` case fails, and the net comes back with its test.)
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
    // (no context till the new one: a refusal while the runtime and context are made touches none freed)
    cx = NULL;
    oom = 0;
#ifdef GLUE_PROBE
    counting = 0;
    grown = compiled_n = 0;
#endif
    calls = 0;
    waiting_n = 0;
    printed_bytes = 0;
    steps = 0;
    stopped = 0;
    syntax = 0;
    rt = JS_NewRuntime2(&glue_heap, NULL);
    JS_SetMemoryLimit(rt, MEMORY_LIMIT);
    JS_SetInterruptHandler(rt, interrupt, NULL);
    // The original's stack limit (`JS_NewRuntime2`: JS_DEFAULT_STACK_SIZE, 1 MB) is measured down from the stack's
    // top (`update_stack_limit`), which with the module's 1 MB stack linked first is below 1 MB: it would wrap below
    // address 0, and every check would say overflow. Set it within that shadow stack (past it is below address 0, a
    // trap), so deep recursion is an error the program sees.
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
#ifdef GLUE_PROBE
    counting = 1;
#endif
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
#ifdef GLUE_PROBE
    compiled_n = grown;
#endif
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
    if (oom) {
        // refused while the answer was read (or before): no JS runs on, the call stays out till `fresh` frees it
        JS_FreeValue(cx, v);
        return out_of_memory();
    }
    JSValue r = JS_Call(cx, resolve[id][err ? 1 : 0], JS_UNDEFINED, 1, (JSValueConst *)&v);
    JS_FreeValue(cx, r);
    JS_FreeValue(cx, v);
    JS_FreeValue(cx, resolve[id][0]);
    JS_FreeValue(cx, resolve[id][1]);
    resolve[id][0] = resolve[id][1] = JS_UNDEFINED;
    return settle();
}
