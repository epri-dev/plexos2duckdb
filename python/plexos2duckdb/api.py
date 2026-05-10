from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
from typing import Any, Callable

import duckdb
from rich.table import Table


class PLEXOS2DuckDBError(RuntimeError):
    """Raised when the plexos2duckdb CLI returns an error."""


class PLEXOS2DuckDB:
    """Python workflow wrapper around the ``plexos2duckdb`` CLI.

    Use this class when you want to start from a PLEXOS solution ZIP or XML
    file, convert it to DuckDB, access conversion metadata, inspect table
    inventory, and run SQL queries against the result from Python.

    Typical workflow:

    1. Construct a client from the source solution file.

       >>> client = PLEXOS2DuckDB("Model DAY_AHEAD Solution.zip")

    2. Convert it to DuckDB. The output path is optional and defaults
        to the input path with a ``.duckdb`` suffix.
        If the output file already exists, the conversion will error out
        unless you pass ``force=True``.

       >>> output_path = client.convert(force=True)

    3. Open the generated database as a context manager for repeated queries.

       >>> with client as db:
       ...     rows = db.query("SELECT COUNT(*) FROM information_schema.tables")

      Alternatively, you can use the `query()` method for one-off queries without managing a connection:

        >>> rows = client.query("SELECT COUNT(*) FROM information_schema.tables")
    """

    def __init__(
        self,
        input_path: str | pathlib.Path | None = None,
        *,
        output_path: str | pathlib.Path | None = None,
        executable: str | pathlib.Path | None = None,
        read_only: bool = True,
    ) -> None:
        self._explicit_executable = executable is not None
        self._executable = (
            pathlib.Path(executable)
            if executable is not None
            else self._resolve_executable()
        )

        self._input_path = pathlib.Path(input_path) if input_path is not None else None
        self._output_path = self._normalize_output_path(output_path, self._input_path)
        self._read_only = read_only
        self._connection: duckdb.DuckDBPyConnection | None = None

    def __enter__(self) -> PLEXOS2DuckDB:
        self.open()
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        self.close()

    @property
    def executable(self) -> pathlib.Path:
        return self._executable

    @property
    def input_path(self) -> pathlib.Path | None:
        return self._input_path

    @property
    def output_path(self) -> pathlib.Path | None:
        return self._output_path

    @property
    def database(self) -> pathlib.Path | None:
        return self._output_path

    @property
    def connection(self) -> duckdb.DuckDBPyConnection:
        if self._connection is None:
            raise PLEXOS2DuckDBError(
                "No managed DuckDB connection is open. Run `convert()` first and then use `with client as db`, call `open()`, or use `connect()` for a temporary connection."
            )
        return self._connection

    @property
    def metadata(self) -> Table:
        return self._build_metadata_table(self._load_metadata())

    @property
    def inventory(self) -> Table:
        return self._build_inventory_table(self._load_inventory())

    def inspect(
        self,
        database: str | pathlib.Path | None = None,
        *,
        format: str = "table",
    ) -> Table | dict[str, Any]:
        database_path = self._resolve_output_path(database)
        completed = self._run_command(
            [
                "inspect",
                "--input",
                str(database_path),
                "--format-diagnostics",
                "json",
            ]
        )
        payload = json.loads(completed.stdout)
        if format == "json":
            return payload
        if format != "table":
            raise PLEXOS2DuckDBError("inspect format must be either 'table' or 'json'")
        return self._build_metadata_table(payload["metadata"])

    def open(
        self,
        database: str | pathlib.Path | None = None,
        *,
        read_only: bool | None = None,
    ) -> duckdb.DuckDBPyConnection:
        database_path = self._resolve_output_path(database)
        if read_only is not None:
            self._read_only = read_only
        if self._connection is None:
            self._connection = duckdb.connect(
                str(database_path), read_only=self._read_only
            )
        return self._connection

    def close(self) -> None:
        if self._connection is not None:
            self._connection.close()
            self._connection = None

    def _repr_items(self) -> list[tuple[str, Any]]:
        items: list[tuple[str, Any]] = []

        if self._input_path is not None:
            items.append(("input_path", str(self._input_path)))
        if self._output_path is not None:
            items.append(("output_path", str(self._output_path)))
        if not self._read_only:
            items.append(("read_only", False))
        if self._explicit_executable:
            items.append(("executable", str(self._executable)))

        return items

    def __repr__(self) -> str:
        body = ", ".join(f"{key}={value!r}" for key, value in self._repr_items())
        return f"{type(self).__name__}({body})"

    def _repr_pretty_(self, printer: Any, cycle: bool) -> None:
        if cycle:
            printer.text(f"{type(self).__name__}(...)")
            return

        items = self._repr_items()
        if not items:
            printer.text(f"{type(self).__name__}()")
            return

        printer.text(f"{type(self).__name__}(")
        with printer.group(2, "", ")"):
            for index, (key, value) in enumerate(items):
                if index:
                    printer.text(",")
                    printer.breakable()
                printer.text(f"{key}=")
                printer.pretty(value)

    def version(self) -> str:
        completed = self._run_command(["--version"])
        return completed.stdout.strip()

    def convert(
        self,
        input_path: str | pathlib.Path | None = None,
        output_path: str | pathlib.Path | None = None,
        *,
        force: bool = False,
        in_memory: bool = False,
        n_threads: int | None = None,
        table_name_pattern: str | None = None,
        on_event: Callable[[dict[str, Any]], None] | None = None,
    ) -> pathlib.Path:
        input_path = self._resolve_input_path(input_path)
        requested_output_path = (
            pathlib.Path(output_path) if output_path is not None else self._output_path
        )
        args = [
            "convert",
            "--input",
            str(input_path),
            "--format-diagnostics",
            "json",
            "--no-progress-bar",
        ]
        if requested_output_path is not None:
            args.extend(["--output", str(requested_output_path)])
        if force:
            args.append("--force")
        if in_memory:
            args.append("--in-memory")
        if n_threads is not None:
            args.extend(["--n-threads", str(n_threads)])
        if table_name_pattern is not None:
            args.extend(["--table-name-pattern", table_name_pattern])

        if on_event is None:
            completed = self._run_command(args)
            events = [
                json.loads(line)
                for line in completed.stdout.splitlines()
                if line.strip()
            ]
        else:
            events = self._run_convert_stream(args, on_event)

        for event in reversed(events):
            if event.get("event") == "completed":
                path = pathlib.Path(event["output"])
                self._input_path = input_path
                self._output_path = path
                return path
        raise PLEXOS2DuckDBError("Conversion did not emit a completion event")

    def connect(
        self, database: str | pathlib.Path | None = None, *, read_only: bool = True
    ) -> duckdb.DuckDBPyConnection:
        database_path = self._resolve_output_path(database)
        return duckdb.connect(str(database_path), read_only=read_only)

    def query(
        self,
        sql: str,
        parameters: list[Any] | tuple[Any, ...] | None = None,
        *,
        database: str | pathlib.Path | None = None,
        read_only: bool = True,
    ) -> list[tuple[Any, ...]]:
        if database is None and self._connection is not None:
            result = self._connection.execute(sql, parameters or [])
            return result.fetchall()

        connection = self.connect(database, read_only=read_only)
        try:
            result = connection.execute(sql, parameters or [])
            return result.fetchall()
        finally:
            connection.close()

    def _resolve_executable(self) -> pathlib.Path:
        import sysconfig

        module_scripts_dir = pathlib.Path(__file__).resolve().parents[2] / "bin"
        candidates = [
            module_scripts_dir / "plexos2duckdb",
            module_scripts_dir / "plexos2duckdb.exe",
            pathlib.Path(sysconfig.get_path("scripts")) / "plexos2duckdb",
            pathlib.Path(sysconfig.get_path("scripts")) / "plexos2duckdb.exe",
        ]
        for candidate in candidates:
            if candidate.exists():
                return candidate
        resolved = shutil.which("plexos2duckdb")
        if resolved is not None:
            return pathlib.Path(resolved)
        raise PLEXOS2DuckDBError(
            "Could not find the bundled `plexos2duckdb` executable or a PATH fallback. Install the package with `pip install plexos2duckdb` or pass `executable=` explicitly."
        )

    def _quote_ident(self, identifier: str) -> str:
        return '"' + identifier.replace('"', '""') + '"'

    def _load_metadata(
        self, database: str | pathlib.Path | None = None
    ) -> dict[str, Any]:
        database_path = self._resolve_output_path(database)
        connection = self.connect(database_path)
        try:
            rows = connection.execute(
                "SELECT key, value FROM main.plexos2duckdb"
            ).fetchall()
        finally:
            connection.close()

        metadata = {key: value for key, value in rows}
        return {
            "database": str(database_path),
            "converter_version": metadata.get("plexos2duckdb_version", ""),
            "source_file": metadata.get("plexos_file", ""),
            "model_name": metadata.get("model_name", ""),
        }

    def _load_inventory(
        self, database: str | pathlib.Path | None = None
    ) -> list[dict[str, Any]]:
        connection = self.connect(database)
        try:
            rows = connection.execute(
                """
                SELECT table_schema, table_name, table_type
                FROM information_schema.tables
                WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
                ORDER BY table_schema, table_name
                """
            ).fetchall()
            inventory = []
            for schema, table_name, table_type in rows:
                kind = "table" if table_type == "BASE TABLE" else "view"
                row_count = "-"
                if table_type == "BASE TABLE":
                    quoted_schema = self._quote_ident(schema)
                    quoted_table = self._quote_ident(table_name)
                    count_row = connection.execute(
                        f"SELECT COUNT(*) FROM {quoted_schema}.{quoted_table}"
                    ).fetchone()
                    if count_row is None:
                        raise PLEXOS2DuckDBError(
                            f"Could not count rows for {quoted_schema}.{quoted_table}"
                        )
                    count = count_row[0]
                    row_count = str(count)
                inventory.append(
                    {
                        "schema": schema,
                        "table": table_name,
                        "kind": kind,
                        "row_count": row_count,
                    }
                )
            return inventory
        finally:
            connection.close()

    def _build_metadata_table(self, metadata: dict[str, Any]) -> Table:
        table = Table(title="PLEXOS2DuckDB Metadata")
        table.add_column("Field")
        table.add_column("Value")
        for key in ("database", "converter_version", "source_file", "model_name"):
            table.add_row(key, str(metadata.get(key, "")))
        return table

    def _build_inventory_table(self, inventory: list[dict[str, Any]]) -> Table:
        table = Table(title="PLEXOS2DuckDB Inventory")
        table.add_column("Schema")
        table.add_column("Table")
        table.add_column("Kind")
        table.add_column("Row Count", justify="right")
        for row in inventory:
            table.add_row(
                str(row.get("schema", "")),
                str(row.get("table", "")),
                str(row.get("kind", "")),
                str(row.get("row_count", "")),
            )
        return table

    def _display_state(self) -> dict[str, Any]:
        return {
            "input_path": str(self._input_path)
            if self._input_path is not None
            else None,
            "output_path": str(self._output_path)
            if self._output_path is not None
            else None,
            "output_exists": self._output_path.exists()
            if self._output_path is not None
            else False,
            "connection_open": self._connection is not None,
            "read_only": self._read_only,
            "executable": str(self._executable),
        }

    def _normalize_output_path(
        self,
        output_path: str | pathlib.Path | None,
        input_path: pathlib.Path | None,
    ) -> pathlib.Path | None:
        if output_path is not None:
            path = pathlib.Path(output_path)
            return path if path.suffix else path.with_suffix(".duckdb")
        if input_path is None:
            return None
        if input_path.suffix.lower() in {".zip", ".xml"}:
            candidate = input_path.with_suffix(".duckdb")
            return candidate if candidate.exists() else None
        return None

    def _resolve_input_path(
        self, input_path: str | pathlib.Path | None
    ) -> pathlib.Path:
        if input_path is not None:
            return pathlib.Path(input_path)
        if self._input_path is not None:
            return self._input_path
        raise PLEXOS2DuckDBError("An input solution path is required for conversion")

    def _resolve_output_path(self, database: str | pathlib.Path | None) -> pathlib.Path:
        if database is not None:
            return pathlib.Path(database)
        if self._output_path is not None:
            return self._output_path
        raise PLEXOS2DuckDBError(
            "An output DuckDB path is required for this operation. Run `convert()` first, pass `output_path=` in the constructor, or provide `database=` explicitly."
        )

    def _run_convert_stream(
        self,
        args: list[str],
        on_event: Callable[[dict[str, Any]], None],
    ) -> list[dict[str, Any]]:
        process = subprocess.Popen(
            [str(self._executable), *args],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        events: list[dict[str, Any]] = []
        stdout = process.stdout
        if stdout is None:
            process.kill()
            raise PLEXOS2DuckDBError("Failed to capture conversion output")

        try:
            for line in stdout:
                line = line.strip()
                if not line:
                    continue
                event = json.loads(line)
                events.append(event)
                on_event(event)
        finally:
            stdout.close()

        stderr = process.stderr.read() if process.stderr is not None else ""
        if process.stderr is not None:
            process.stderr.close()
        return_code = process.wait()
        if return_code != 0:
            message = stderr.strip() or "Unknown error"
            raise PLEXOS2DuckDBError(message)
        return events

    def _run_command(self, args: list[str]) -> subprocess.CompletedProcess[str]:
        completed = subprocess.run(
            [str(self._executable), *args],
            text=True,
            capture_output=True,
            check=False,
        )
        if completed.returncode != 0:
            message = (
                completed.stderr.strip() or completed.stdout.strip() or "Unknown error"
            )
            raise PLEXOS2DuckDBError(message)
        return completed
