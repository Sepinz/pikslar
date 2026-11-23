# The scripting language

Phase 12.4, part a. The crate `pikslar-script` is Pikslar's own interpreter, chosen by the owner after the spike (`docs/m12-interpreter-spike.md`). It is plain Rust with no dependencies. Part b binds it to `pikslar-api` and the app through the crate `pikslar-bind` (`docs/scripting-api.md`, Script binding).

## Shape

- `compile(source, &HostTable, &Limits) -> Result<Program, ScriptError>`. The host table lists the only functions a script may call outside the language, with their argument counts.
- `run(&Program, &mut impl Host, &Limits, &AtomicBool) -> RunReport`. The report holds the result and what the run used: fuel, peak memory and native stack.
- A `Program` holds no shared values, so it is `Send + Sync`. It can be compiled on one thread and run on another, any number of times.
- Runs must be on a thread with `RECOMMENDED_STACK` (16 MiB) of stack. A host needs a worker thread anyway, so that cancelling works.

## Syntax

```
# A comment runs to the end of the line.
fn shade(c, k) {                        # Functions are defined at the top level only.
    return c * k / 4;
}

let w = 16;                             # Declares a variable in the current block.
w = w + 1;                              # Assigns to it.
let row = list(w, 0);                   # A list of w zeros.
for x in 0..w {                         # Range: from 0 up to, not including, w.
    row[x] = shade(x, 3);
}
for v in [1, 2, 3] { }                  # Loops over a list's items.
while true { break; }                   # Also `continue`.
if w > 10 { } else if w == 0 { } else { }
return row;                             # Ends the run with this value.
```

- Literals: integers (`12`, `0xff00ff`), floats (`1.5`, which need a digit after the point; a literal past the largest float is a `Syntax` error, so a float literal is always finite), strings (`"a\n"`, with the escapes `\n \t \" \\`), `true`, `false`, `nil`, and lists `[a, b]`.
- Operators, loosest first: `||`, `&&`, comparisons (`== != < <= > >=`), `+ -`, `* / %`, unary `- !`, indexing `l[i]`. Comparisons do not chain: `a < b < c` is a syntax error.
- Statements end with `;`. Blocks are braced and always required.
- Names are ASCII letters, digits and `_`. Outside strings and comments, source must be ASCII.

## Semantics

- **Values.** `nil`, booleans, 64-bit integers, 64-bit floats, strings and lists.
- **Integers** are checked. Overflow is an `Arithmetic` error, never a wrap. `/` and `%` are Euclidean: `-7 / 2` is `-4`, `-7 % 2` is `1`, so `x % w` always wraps a coordinate into `0..w`. Division by zero is an error.
- **Floats.** An integer meeting a float becomes a float. A result that is not finite (infinity or NaN) is an `Arithmetic` error, so NaN never exists. Floats print in Rust's `Debug` form (`1.0`, `1e300`), which always shows they are floats.
- **Strings** are immutable UTF-8. `+` joins two strings only. Use `str()` to turn anything else into one. `len`, `slice` and `find` count bytes. A slice that is not on a character boundary is an `Index` error.
- **Lists** are shared by reference: `let b = a;` gives two names for one list. They hold scalars and strings only, never lists. Putting a list into a list is a `Type` error. This is deliberate. Without nesting, values cannot form cycles, so nothing leaks and no reference-count cycle is needed. Dropping, comparing and printing never recurse, so no hostile structure can overflow the stack. A grid is a flat list indexed `x + y * w`. Equality `==` on lists compares items. `for v in list` reads the length again each turn, so items the body adds are visited.
- **Conditions** (`if`, `while`, `&&`, `||`, `!`) must be booleans. There is no truthiness: `if 1` is a `Type` error. `&&` and `||` short-circuit.
- **Equality** between different types is `false`, except that integers and floats compare by value. Ordering is only between numbers or between strings (by bytes). Anything else is a `Type` error.
- **Scopes.** `let` declares in the current block. It may shadow, and `let x = x + 1;` reads the outer `x`. Variables are resolved to frame slots at compile time. Using an undeclared name is a compile error.
- **Functions** see only their parameters and their own locals, never top-level variables. There are no closures and no function values. They can be called before they are defined, and they can recurse. A function without `return` gives `nil`.
- **Determinism.** No hashing reaches script behaviour (the compiler's tables are ordered). There is no randomness, time, locale or platform maths: `sqrt`, `floor` and `round` are exactly rounded IEEE operations. The same program and host always give the same result.

### Builtins

| Builtin | Does | Fuel, besides 1 |
|---|---|---|
| `len(s or l)` | Bytes of a string, items of a list | |
| `push(l, v)`, `pop(l)` | Add at the end, remove from the end | one per item when the list grows |
| `insert(l, i, v)`, `remove(l, i)` | At place `i` | one per item |
| `slice(s or l, from, to)` | Copy of `from` up to `to` | per 16 bytes or per item |
| `find(s, sub)`, `find(l, v)` | First place, or -1 | per 16 bytes of both, or per item |
| `repeat(s, n)` | `s` n times | per 16 bytes made, and at least per 16 turns: `repeat("", n)` pays for its `n` turns though it makes nothing |
| `list(n, v)` | `n` copies of `v` | per item |
| `str(v)`, `int(v)`, `float(v)` | Conversions; `int` truncates, and strings must parse exactly | per 16 bytes |
| `type(v)` | `"nil"`, `"bool"`, `"int"`, `"float"`, `"string"` or `"list"` | |
| `abs`, `min`, `max`, `floor`, `round`, `sqrt` | Numbers; `floor` and `round` give integers | |

String search uses the standard library's two-way algorithm. It is linear in both lengths and allocates nothing. There is no pattern or regular-expression engine.

## What is deliberately absent

No file, network, process, environment, clock, randomness or system access exists in the language: there is no name for any of them. The only way out is a host function the embedder adds to the `HostTable` (capability passing). Also absent: maps (a deterministic, metered map is possible later, if a need appears), nested lists, closures, function values, modules and imports, `eval`, string indexing, exceptions and catching errors (a script cannot catch a fuel or memory stop), and integer wrapping.

## Errors

Every error is a `ScriptError { kind, pos: {line, column}, message }`. The column counts characters. It prints as `line 3, column 13: division by zero`. Runtime errors carry the place of the operator, call or index that failed.

Script data in a message is cut short. A quoted name or text is at most 64 characters, then an ellipsis (`error::quoted`), and a whole message is at most 512 bytes, cut at a character boundary with an ellipsis (`error::capped`, applied by every syntax and runtime error and by `HostError::new`). A script that names a megabyte-long function gets a short error, not a megabyte of it.

| Kind | Meaning |
|---|---|
| `Syntax` | Does not parse, an unknown name, a wrong argument count, `break` outside a loop |
| `TooLarge` | Over the source size limit, the nesting limit, the locals or functions limit, or the memory cap at compile time |
| `Type`, `Arithmetic`, `Index`, `Value` | Script mistakes at run time |
| `Host` | A host function's own error |
| `Fuel`, `Memory`, `AllocFailed`, `CallDepth`, `Cancelled` | Stops, by a limit or by the host. `ErrorKind::is_stop` tells them from mistakes |

## Limits

| Limit | Default | Kind when hit |
|---|---|---|
| Fuel per run | 50,000,000 units | `Fuel` |
| Live memory, the program included | 16 MiB | `Memory` |
| Call depth | 128 (at most 256) | `CallDepth` |
| Native stack (`STACK_BUDGET`) | 8 MiB, checked before each call | `CallDepth` |
| Source | 256 KiB | `TooLarge` |
| Nesting of blocks and expressions together | 64 | `TooLarge` |
| Locals per function, arguments, functions | 256, 16, 1,024 | `TooLarge` |
| Cancel flag | read at least every 1,024 fuel units | `Cancelled` |

**Fuel.** Each statement burns one unit plus one for each expression node in it, counted at compile time. That costs nothing extra at run time, and the work between two burns is bounded by the size of one statement. Each loop turn burns one unit, plus the condition's nodes for `while`. A call to a script function burns one unit for each local it makes beyond its arguments, since it fills them with `nil` and drops them on return: a function of 255 locals that returns at once was otherwise about 80 times the empty loop's work per unit, and now stops at default fuel in 0.27 s (`hostile_calls`). Builtins burn in proportion to the bytes or items they touch, and to the turns of any loop they run, before doing the work. Host functions get `HostCx::charge(units)` and must pay for work that grows with its input. They make lists through `HostCx::new_int_list`, which charges fuel and memory for the items before it allocates. `HostCx::list_fits` tells whether such a list would fit the memory left, so a host can refuse before doing the work. The test canvas charges one unit per 16 pixels of `fill_rect`. Burning more fuel than is left burns nothing and stops the run. A cancel is seen within 1,024 units of work, and within one builtin or host step. That holds because no builtin or host step does more work than it burned for, loop turns included (`builtins_pay_for_their_loop_counts_not_only_their_bytes` runs each with a large count and must stop within a second). A step that burned a lot can still run to its end before the flag is read: the largest, a 16 MiB string, takes milliseconds.

**Nesting** counts each operator added to a chain, so `1 + 1 + ... + 1` is as deep as it is long. This bounds the interpreter's recursion within one function, and the recursion of dropping the tree. Between functions, the call depth and the native stack budget bound it. A run measures its own stack depth at each block and before each call, so the bound holds whatever the build's frame sizes.

## Memory accounting

### Why the spike's peak passed its charge

Measured on 2026-11-16 by instrumenting the spike in a scratch copy (a counting allocator plus the Win32 process counters), on its 100,000-item list:

| | Bytes |
|---|---|
| Charged by the spike | 3,145,776 |
| Heap peak, the old and new blocks of a reallocation counted together | 4,720,568 (1.50 times) |
| Process peak private bytes over the start | 6,377,472 (2.03 times) |
| Process private bytes after the run, everything freed | 1,609,728 over the start |
| Parse tree | 1,832 |
| Allocations | 56, of which 31 were reallocations |

The causes, largest first:

1. **Reallocation holds two buffers.** The spike charged only the capacity each doubling added. While a `Vec` grows, the heap holds the old buffer and the new one together during the copy, so the true peak is 1.5 times the final capacity.
2. **The heap keeps freed blocks.** Blocks under the Windows heap's large-block threshold stay committed to the process after they are freed. The earlier, smaller list buffers made up most of the 1.6 MB still committed after the run.
3. **Not significant here:** the parse tree (2 KB), per-allocation headers (56 allocations), call frames and temporaries. The spike's per-value size (24 bytes, from a fat `Rc<str>`) was charged correctly.

### The model now

- **Live, not cumulative.** Every string and list holds a reference to the run's meter and refunds its charge when it is dropped. Churn (making and dropping a megabyte string 200 times) peaks at 2 MB of charge, where a cumulative counter would stop it.
- **Charged before allocating, by capacity, with overhead.** `alloc_charge(n)` rounds to the heap's 16-byte granule and adds 32 bytes of header, below 256 KiB. At and above 256 KiB it rounds to 4 KiB pages and adds a spare page. Sizes are computed with checked arithmetic, and an impossible size is a `Memory` stop. The charge is made first. Only then is the allocation made, with `try_reserve_exact`. A refused allocation refunds the charge and stops the run with `AllocFailed`.
- **Growth charges both buffers.** A list or the value stack grows by doubling. The new buffer is charged in full while the old one is still charged, and the old charge is refunded after the copy. This covers cause 1 exactly.
- **Everything is counted:** each string's shared box and text, each list's box and buffer, the value stack that holds every call frame's locals (so deep recursion that holds memory counts), each run's copy of the string literals, and the program itself. The tree is charged 6 nodes' worth per node, and test shapes use a fifth to a tenth of that. It is charged at compile time and again against every run's cap. The source size and nesting are bounded as well.
- **Outside the meter:** the meter and run bookkeeping (well under 1 KiB), an error message, and the native stack, which has its own budget.

### Measured

`tests/memory_cap.rs` counts this thread's heap, from before compiling, with a 4 MiB cap. A reallocation counts its old and new blocks together. It asserts that the heap peak is at most the cap plus 16 KiB, and that the meter never passes the cap. Release build:

| Hostile script | Stopped by | Meter peak | Heap peak, of the cap |
|---|---|---|---|
| List growth | `Memory` | 3,195,696 | 0.75 |
| String doubling | `Memory` | 3,196,496 | 0.75 |
| Many small strings | `Memory` | 4,194,272 | 0.49 |
| Many short lists, rendered | `Memory` | 4,194,256 | 0.50 |
| Recursion, each frame holding a 320 KB list | `Memory` | 3,978,880 | 0.92 |
| Single allocations of 10^12 bytes, of `i64::MAX` times, and just over the cap | `Memory`, before allocating | about 40,000 | under 2 KB allocated |
| Churn, 200 megabyte strings | ran to completion | 2,064,512 | 0.48 |

The heap stays below the meter because the charge model is conservative: about half for many small values.

In separate processes (`examples/probe.rs hostile`, release, 16 MiB cap, fuel 10,000,000), peak private bytes over the start includes the host's 256 KB canvas:

| Script | Stopped by | ms | Peak private over the start, KB | Of the cap |
|---|---|---|---|---|
| Infinite loop | `Fuel` | 30.2 | 260 | 0.02 |
| Loop in nested functions | `Fuel` | 34.3 | 260 | 0.02 |
| String doubling | `Memory` | 3.9 | 13,664 | 0.83 |
| Unbounded list | `Memory` | 24.0 | 13,672 | 0.83 |
| Many small strings | `Memory` | 21.1 | 10,784 | 0.66 |
| Deep recursion | `CallDepth` | 0.2 | 424 | 0.03 |
| Frames each holding 2.2 MB | `Memory` | 8.3 | 15,628 | 0.95 |
| Huge single allocation | `Memory` | 0.1 | 284 | 0.02 |
| Churn | `Fuel` | 3.2 | 2,232 | 0.14 |
| 50,000 small strings freed, then one 15 MB string | completed | 25.7 | 15,160 | 0.93 |
| Search a long string | `Fuel` | 102.0 | 1,248 | 0.08 |
| Quadratic list work | `Fuel` | 10.5 | 5,028 | 0.31 |
| Bulk host work | `Fuel` | 8.6 | 260 | 0.02 |
| Open a file | `Syntax`, no such function | 0.1 | 260 | 0.02 |

**Caveat, cause 2.** Retention is the allocator's, not the interpreter's. If a script frees many small values and then makes one large one, the process can briefly hold both, even though live memory never passed the cap. The "small then large" case reached 0.93 of the cap here, not over it. A strict process-level guarantee would need the binding phase to give script runs a private heap that is destroyed after each run. That cannot be done in this OS-free crate.

**Allocation failure.** `tests/alloc_failure.rs` installs an allocator that refuses every request of 1 MiB or more. List growth, `list`, `repeat`, string doubling, and list growth after a large `slice` each end with `AllocFailed` at the right line and column, with no panic or abort. The next run starts clean. Small fixed-size boxes (the header of a string or list, under 100 bytes) use the standard infallible allocation, as all Rust code does.

## Execution limits, measured

`tests/execution_limits.rs`, release build, fuel 10,000,000. Each stop time is from the start of the run:

| Script | Stopped by | ms |
|---|---|---|
| `while true { }` | `Fuel` | 28.1 |
| A 2^63 range loop | `Fuel` | 65.8 |
| An infinite loop three functions down | `Fuel` | 30.8 |
| `continue` loop | `Fuel` | 35.9 |
| Search a megabyte string, repeatedly | `Fuel` | 99.0 |
| Compare, order or slice megabyte strings | `Fuel` | 6.4 to 6.6 |
| `str` of a megabyte string, and `int` | `Fuel` | 51.1 |
| Insert and remove at the front of 100,000 items | `Fuel` | 11.1 |
| Compare, search, slice or render 100,000 items | `Fuel` | 15.3 to 175.5 |
| `fill_rect` of the whole canvas, repeatedly (host-charged) | `Fuel` | 8.6 |
| `for v in l { push(l, v); }` | `Memory` | 21.6 |
| Unbounded recursion, direct or mutual | `CallDepth` | 0.1 |
| Cancel flag set during an endless loop | `Cancelled`, 0.040 ms after the flag | |

The slowest work per fuel unit is rendering lists (about 18 ns a unit), so a 50,000,000 unit allowance ends within about a second. In the worst case the limits allow (256 calls, each nested 58 deep), the native stack budget stops the run at 7.6 MB in release and 8.4 MB unoptimised, inside the 16 MiB recommended stack.

## Speed

`examples/probe.rs bench`, release, median of 11, against the spike's 7.0 ms:

| Script | ms |
|---|---|
| 256 x 256 `set_pixel`, spike's `while` form | 5.83 |
| The same with `for` ranges | 3.63 |
| 100,000 `push`es | 4.43 |
| One `fill_rect` of the canvas | 0.004 |
| Compiling the pixel script | 0.006 |

These are reports, not baselines. Proposed bench lines, not recorded: `script_pixel_while_ms` and `script_pixel_for_ms`.

## Size

`examples/size_with.rs` (a host that compiles and runs a script with one host function) is 396,288 bytes with the release profile. `examples/size_without.rs` (the same host shape, no interpreter) is 272,384 bytes. The difference, **121 KB**, is an upper bound on what the interpreter adds: some of the formatting code it pulls in, the program already has. The spike's own interpreter added 70 KB, with no `for`, `break`, error positions, live accounting or builtins beyond six.

## Evidence

| File | Tests |
|---|---|
| Unit tests in `src/` | 34: lexer, limits, meter, values, fuel and cancel, host table, operators, builtins, conversions |
| `tests/language.rs` | 21: syntax, semantics, scopes, functions, lists, errors with places, float literals past the largest float, script data cut short in errors, no ambient access, determinism, `Send` programs |
| `tests/memory_cap.rs` | 9: the hostile scripts above, the parse tree, source and nesting bounds |
| `tests/alloc_failure.rs` | 2: failure paths, and the next run |
| `tests/execution_limits.rs` | 11: fuel, metered builtins and their loop counts, host charging, depth, worst-case stack, cancel |
| `tests/mutation.rs` | 1: 20,000 mutated scripts, none panics; 790 compile and run |

Run the measurements with `--nocapture`. On this machine the release test binaries' paths are too long to start through `cargo test --release`, so they were copied to a short path and run there.

## Open questions

- A process-level hard cap would need a per-run private heap. The binding decided against one: see `docs/scripting-api.md`, Memory. Measured in the app's build, the worst case (small strings freed, then one large string) reached 0.91 of the limit.
- Maps and nested lists are left out. If scripts need structured data, a value-semantics nested list with a depth limit, or a sorted-key map, can be added with the same accounting.
- The default limits (fuel 50,000,000, memory 16 MiB, depth 128) are proposals for the owner. They are now settings (`script.fuel`, `script.memory_mb`, `script.call_depth`).
