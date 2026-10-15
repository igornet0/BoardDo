# BoardDo tool harness. Reads {"source", "input", "permissions", "limits"} from stdin,
# writes exactly one JSON line with the result to the original stdout; everything the
# tool prints goes to stderr (logs).
#
# Defence in depth for a plain subprocess: a static AST check rejects escape hatches
# (eval/exec, dunder attributes, importlib, ctypes, ...) and an audit hook, which tool
# code cannot remove, enforces the permissions at runtime (processes, network, files).
import ast
import json
import os
import sys
import traceback

try:
    import resource
except ImportError:  # Windows
    resource = None

RESULT = os.fdopen(os.dup(1), "w")
sys.stdout = sys.stderr

ALWAYS_BLOCKED_MODULES = {
    "ctypes", "_ctypes", "cffi", "importlib", "imp", "builtins", "__builtin__", "gc",
    "inspect", "code", "codeop", "marshal", "pickle", "_pickle", "shelve", "dill",
    "multiprocessing", "pty", "tty", "fcntl", "posix", "nt", "_posixsubprocess",
    "_thread", "sysconfig", "site", "runpy", "pdb", "trace", "faulthandler",
}
NETWORK_MODULES = {
    "socket", "_socket", "ssl", "http", "urllib", "urllib3", "requests", "httpx",
    "aiohttp", "ftplib", "smtplib", "poplib", "imaplib", "telnetlib", "xmlrpc",
    "socketserver", "asyncio", "websocket", "websockets",
}
SHELL_MODULES = {"subprocess", "signal"}
# Always rejected when read.
BLOCKED_NAMES = {"eval", "exec", "__import__", "globals", "locals", "breakpoint", "__builtins__", "__loader__", "__spec__"}
# Rejected only as the builtin (a parameter or variable with this name is fine).
BLOCKED_BUILTINS = {"compile", "vars", "input", "help", "exit", "quit", "open_code"}
# Reaching native code through a library (numpy.ctypeslib.ctypes, ...).
FORBIDDEN_ATTRS = {"ctypes", "ctypeslib", "_ctypes", "cffi", "pythonapi", "_cffi_backend"}
SAFE_DUNDERS = {
    "__init__", "__name__", "__doc__", "__qualname__", "__module__", "__version__",
    "__str__", "__repr__", "__len__", "__iter__", "__next__", "__contains__",
    "__eq__", "__ne__", "__lt__", "__le__", "__gt__", "__ge__", "__hash__",
    "__add__", "__sub__", "__mul__", "__truediv__", "__floordiv__", "__mod__",
    "__getitem__", "__setitem__", "__enter__", "__exit__", "__post_init__",
    "__annotations__", "__slots__", "__all__", "__file__",
}


def safe_builtins():
    """Builtins for the tool: getattr & co refuse dunder names computed at runtime."""
    import builtins as _b

    table = dict(vars(_b))

    def guard(name):
        if isinstance(name, str) and (
            (name.startswith("__") and name.endswith("__") and name not in SAFE_DUNDERS) or name in FORBIDDEN_ATTRS
        ):
            raise PermissionError("attribute `%s` is not allowed" % name)

    def _getattr(obj, name, *default):
        guard(name)
        return _b.getattr(obj, name, *default)

    def _setattr(obj, name, value):
        guard(name)
        return _b.setattr(obj, name, value)

    def _delattr(obj, name):
        guard(name)
        return _b.delattr(obj, name)

    def _hasattr(obj, name):
        guard(name)
        return _b.hasattr(obj, name)

    table.update(getattr=_getattr, setattr=_setattr, delattr=_delattr, hasattr=_hasattr)
    for name in ("eval", "exec", "compile", "__import__", "globals", "locals", "vars", "breakpoint", "input", "help", "exit", "quit", "open_code"):
        table.pop(name, None)
    # `import` statements still need the import machinery.
    table["__import__"] = _b.__import__
    return table


class Violation(Exception):
    pass


def check_source(tree, perms):
    blocked = set(ALWAYS_BLOCKED_MODULES)
    if not perms.get("network"):
        blocked |= NETWORK_MODULES
    if not perms.get("shell"):
        blocked |= SHELL_MODULES

    problems = []

    def bad(node, what):
        problems.append("line %s: %s" % (getattr(node, "lineno", "?"), what))

    bound = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.arg):
            bound.add(node.arg)
        elif isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store):
            bound.add(node.id)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            bound.add(node.name)
        elif isinstance(node, (ast.Import, ast.ImportFrom)):
            bound.update((a.asname or a.name).split(".")[0] for a in node.names)

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                parts = alias.name.split(".")
                if parts[0] in blocked or FORBIDDEN_ATTRS.intersection(parts):
                    bad(node, "import of `%s` is not allowed" % alias.name)
        elif isinstance(node, ast.ImportFrom):
            parts = (node.module or "").split(".")
            if node.level == 0 and (parts[0] in blocked or FORBIDDEN_ATTRS.intersection(parts)):
                bad(node, "import from `%s` is not allowed" % node.module)
            for alias in node.names:
                if alias.name in FORBIDDEN_ATTRS:
                    bad(node, "`from %s import %s` is not allowed" % (node.module, alias.name))
            if parts[0] == "os":
                for alias in node.names:
                    if alias.name in ("system", "popen", "fork", "forkpty", "kill", "killpg") or alias.name.startswith(("exec", "spawn", "posix_spawn")):
                        if not perms.get("shell"):
                            bad(node, "`from os import %s` is not allowed" % alias.name)
        elif isinstance(node, ast.Name) and isinstance(node.ctx, ast.Load):
            if node.id in BLOCKED_NAMES or (node.id in BLOCKED_BUILTINS and node.id not in bound):
                bad(node, "`%s` is not allowed" % node.id)
        elif isinstance(node, ast.Attribute):
            if node.attr in FORBIDDEN_ATTRS:
                bad(node, "attribute `%s` is not allowed" % node.attr)
            if node.attr.startswith("__") and node.attr.endswith("__") and node.attr not in SAFE_DUNDERS:
                bad(node, "attribute `%s` is not allowed" % node.attr)
            if node.attr.startswith("_") and isinstance(node.value, ast.Name) and node.value.id in ("os", "sys"):
                bad(node, "`%s.%s` is not allowed" % (node.value.id, node.attr))
            if isinstance(node.value, ast.Name) and node.value.id == "sys" and node.attr in ("modules", "meta_path", "path_hooks", "settrace", "setprofile", "addaudithook"):
                bad(node, "`sys.%s` is not allowed" % node.attr)
    if problems:
        raise Violation("; ".join(problems[:10]))


def install_audit_hook(perms, workdir):
    allow_read_all = bool(perms.get("filesystem_read"))
    allow_write_all = bool(perms.get("filesystem_write"))
    network = bool(perms.get("network"))
    shell = bool(perms.get("shell"))
    roots = tuple(
        os.path.realpath(p) + os.sep
        for p in {sys.prefix, sys.base_prefix, sys.exec_prefix, sys.base_exec_prefix, os.path.dirname(os.__file__)}
        | set(p for p in sys.path if p and os.path.isdir(p))
    )
    work = os.path.realpath(workdir) + os.sep
    devices = ("/dev/null", "/dev/urandom", "/dev/random")

    def deny(event, detail=""):
        raise PermissionError("blocked by tool permissions: %s %s" % (event, detail))

    def inside(path, prefixes):
        try:
            real = os.path.realpath(path if isinstance(path, str) else os.fsdecode(path))
        except Exception:
            return False
        return real in devices or (real + os.sep).startswith(prefixes)

    def hook(event, args):
        if event == "open":
            path, mode = args[0], args[1] or "r"
            if isinstance(path, int):
                return
            writing = any(c in str(mode) for c in "wax+") or (isinstance(args[2], int) and args[2] & (os.O_WRONLY | os.O_RDWR | os.O_CREAT))
            if writing:
                if not allow_write_all and not inside(path, (work,)):
                    deny("write", path)
            elif not allow_read_all and not inside(path, roots + (work,)):
                deny("read", path)
        elif event in ("os.listdir", "os.scandir", "glob.glob"):
            path = args[0] if args else "."
            if not allow_read_all and not inside(path if path is not None else ".", roots + (work,)):
                deny(event, path)
        elif event in ("os.remove", "os.unlink", "os.rmdir", "os.rename", "os.replace", "os.mkdir", "os.chmod", "os.chown", "os.link", "os.symlink", "os.truncate", "shutil.rmtree", "shutil.move", "os.utime"):
            if not allow_write_all and not all(inside(a, (work,)) for a in args[:2] if isinstance(a, (str, bytes))):
                deny(event, args[0] if args else "")
        elif event in ("subprocess.Popen", "os.system", "os.exec", "os.spawn", "os.posix_spawn", "os.fork", "os.forkpty", "os.kill", "os.killpg", "pty.spawn"):
            if not shell:
                deny(event)
        elif event.startswith("socket.") and event not in ("socket.__new__",):
            if not network:
                deny(event)
        elif event in ("urllib.Request", "ftplib.connect", "smtplib.connect", "http.client.connect"):
            if not network:
                deny(event)
        elif event in ("ctypes.dlsym", "ctypes.dlsym/handle", "ctypes.call_function", "ctypes.cdata", "ctypes.cdata/buffer", "ctypes.string_at", "ctypes.wstring_at", "ctypes.set_errno"):
            # Libraries such as numpy import ctypes; looking up / calling native
            # functions or reading raw memory is what is denied.
            deny(event)
        elif event in ("sys.setprofile", "sys.settrace", "sys.addaudithook", "code.__new__"):
            deny(event)

    sys.addaudithook(hook)

    # Older interpreters (3.8) raise no audit event for some of these: replace them.
    # Replacements are callable objects, not functions, so modules that keep
    # `os.x` as a class attribute (pathlib) do not bind them as methods.
    def guarded(fn):
        class Guarded:
            __slots__ = ()

            def __call__(self, *args, **kwargs):
                return fn(*args, **kwargs)

            def __repr__(self):
                return "<guarded os function>"

        return Guarded()

    def blocked(name):
        def stub(*args, **kwargs):
            raise PermissionError("blocked by tool permissions: os.%s" % name)
        return guarded(stub)

    if not shell:
        for name in (
            "system", "popen", "fork", "forkpty", "kill", "killpg", "startfile",
            "execl", "execle", "execlp", "execlpe", "execv", "execve", "execvp", "execvpe",
            "spawnl", "spawnle", "spawnlp", "spawnlpe", "spawnv", "spawnve", "spawnvp", "spawnvpe",
            "posix_spawn", "posix_spawnp",
        ):
            if hasattr(os, name):
                setattr(os, name, blocked(name))
    if not allow_write_all:
        real = {}
        for name in ("remove", "unlink", "rmdir", "rename", "replace", "mkdir", "makedirs", "removedirs", "chmod", "chown", "link", "symlink", "truncate", "utime"):
            if hasattr(os, name):
                real[name] = getattr(os, name)

        def gated(name):
            fn = real[name]

            def wrapper(*args, **kwargs):
                for a in args[:2]:
                    if isinstance(a, (str, bytes)) and not inside(a, (work,)):
                        raise PermissionError("blocked by tool permissions: os.%s %s" % (name, a))
                return fn(*args, **kwargs)
            return guarded(wrapper)

        for name in real:
            setattr(os, name, gated(name))
    if not allow_read_all:
        for name in ("listdir", "scandir"):
            fn = getattr(os, name)

            def make(fn, name):
                def wrapper(path=".", *args, **kwargs):
                    if isinstance(path, (str, bytes)) and not inside(path, roots + (work,)):
                        raise PermissionError("blocked by tool permissions: os.%s %s" % (name, path))
                    return fn(path, *args, **kwargs)
                return guarded(wrapper)
            setattr(os, name, make(fn, name))


def set_limits(limits):
    if resource is None:
        return
    mb = int(limits.get("memory_mb") or 0)
    if mb > 0 and hasattr(resource, "RLIMIT_AS") and sys.platform.startswith("linux"):
        size = mb * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_AS, (size, size))
    secs = int(limits.get("timeout_secs") or 0)
    if secs > 0:
        try:
            resource.setrlimit(resource.RLIMIT_CPU, (secs + 1, secs + 2))
        except (ValueError, OSError):
            pass


def emit(obj):
    RESULT.write(json.dumps(obj, ensure_ascii=False) + "\n")
    RESULT.flush()


def fail(kind, message, tb=None):
    emit({"success": False, "error": {"type": kind, "message": message, "traceback": tb}})


def user_traceback():
    """Traceback limited to the tool's own frames."""
    etype, value, tb = sys.exc_info()
    frames = [f for f in traceback.extract_tb(tb) if f.filename == "tool.py"]
    lines = ["Traceback (most recent call last):"] + traceback.format_list(frames)
    lines += traceback.format_exception_only(etype, value)
    return "".join(l if l.endswith("\n") else l + "\n" for l in lines).rstrip()


def main():
    envelope = json.loads(sys.stdin.read() or "{}")
    source = envelope.get("source", "")
    perms = envelope.get("permissions") or {}
    limits = envelope.get("limits") or {}
    mode = envelope.get("mode", "run")

    try:
        tree = ast.parse(source, "tool.py")
    except SyntaxError as e:
        line = (e.text or "").rstrip()
        return fail("SyntaxError", "line %s: %s%s" % (e.lineno, e.msg, (" — " + line.strip()) if line else ""))
    try:
        check_source(tree, perms)
    except Violation as e:
        return fail("SecurityError", str(e))
    if not any(isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and n.name == "run" for n in tree.body):
        return fail("DefinitionError", "the tool must define a top-level function `run(input_data)`")
    if mode == "check":
        return emit({"success": True, "output": None})

    set_limits(limits)
    install_audit_hook(perms, os.getcwd())
    namespace = {"__name__": "tool", "__file__": "tool.py", "__builtins__": safe_builtins()}
    try:
        exec(compile(tree, "tool.py", "exec"), namespace)
    except ModuleNotFoundError as e:
        return fail("ModuleNotFoundError", "module `%s` is not installed for the tool runtime (%s)" % (e.name, sys.executable), user_traceback())
    except BaseException as e:
        return fail(type(e).__name__, "while loading the tool: %s" % e, user_traceback())

    run = namespace.get("run")
    try:
        result = run(envelope.get("input"))
    except MemoryError:
        return fail("MemoryError", "the tool exceeded its memory limit (%s MB)" % limits.get("memory_mb"))
    except RecursionError as e:
        return fail("RecursionError", str(e), user_traceback())
    except SystemExit as e:
        return fail("SystemExit", "the tool called exit(%s)" % e.code)
    except BaseException as e:
        return fail(type(e).__name__, str(e), user_traceback())

    try:
        payload = json.dumps({"success": True, "output": result}, ensure_ascii=False, allow_nan=False)
    except (TypeError, ValueError) as e:
        return fail("OutputError", "the result is not JSON-serializable: %s (return dict/list/str/int/float/bool/None)" % e)
    RESULT.write(payload + "\n")
    RESULT.flush()


if __name__ == "__main__":
    try:
        main()
    except BaseException as e:  # never leave the caller without a result line
        fail("HarnessError", "%s: %s" % (type(e).__name__, e), traceback.format_exc())
