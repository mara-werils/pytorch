import json
import multiprocessing
import os
import sys
import traceback


def _suppress_stdout():
    """Redirect fd 1 (stdout) to /dev/null at the OS level.

    This catches C-level output (e.g. ninja, setuptools) that bypasses
    Python's sys.stdout.  Returns the saved fd so it can be restored.
    """
    saved = os.dup(1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 1)
    os.close(devnull)
    return saved


def _restore_stdout(saved_fd):
    os.dup2(saved_fd, 1)
    os.close(saved_fd)


def collect_from_file(args):
    file_path, repo_root = args
    rel_path = os.path.relpath(file_path, repo_root)
    collected = []

    saved_fd = _suppress_stdout()
    saved_stderr = os.dup(2)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 2)
    os.close(devnull)

    try:
        import pytest

        class CollectorPlugin:
            @staticmethod
            def pytest_collection_finish(session):
                for item in session.items:
                    collected.append(item.nodeid)

        exit_code = pytest.main(
            ["--collect-only", "-q", file_path],
            plugins=[CollectorPlugin()],
        )

        error = None if exit_code in (0, 5) else f"pytest exit {exit_code}"
        return {"file": rel_path, "tests": collected, "error": error}

    except SystemExit:
        return {"file": rel_path, "tests": [], "error": None}
    except Exception:
        msg = traceback.format_exc().splitlines()[-1]
        return {"file": rel_path, "tests": [], "error": msg}
    finally:
        _restore_stdout(saved_fd)
        os.dup2(saved_stderr, 2)
        os.close(saved_stderr)


def main():
    input_data = json.loads(sys.stdin.read())
    files = input_data["files"]
    repo_root = input_data["repo_root"]
    jobs = input_data["jobs"]
    output_path = input_data["output"]

    sys.stderr.write("importing torch... ")
    sys.stderr.flush()
    import torch  # noqa: F401
    import pytest  # noqa: F401

    sys.stderr.write("done\n")
    sys.stderr.flush()

    multiprocessing.set_start_method("fork")

    with multiprocessing.Pool(processes=jobs, maxtasksperchild=1) as pool:
        results = pool.map(collect_from_file, [(f, repo_root) for f in files])

    with open(output_path, "w") as f:
        json.dump({"results": results}, f)


if __name__ == "__main__":
    main()
