"""Tests for the ``PLEXOS2DuckDB.report`` dot-accessor.

These tests use the pre-built ``Model_Base_LT_Solution.duckdb`` fixture
under ``tests/data`` so they do not depend on running the Rust converter.
"""

from __future__ import annotations

import pathlib

import duckdb
import pytest

import plexos2duckdb
from plexos2duckdb import api as api_module


FIXTURE_DB = (
    pathlib.Path(__file__).resolve().parents[2]
    / "tests"
    / "data"
    / "Model_Base_LT_Solution.duckdb"
)


@pytest.fixture
def client() -> plexos2duckdb.PLEXOS2DuckDB:
    if not FIXTURE_DB.exists():
        pytest.skip(f"missing fixture: {FIXTURE_DB}")
    return plexos2duckdb.PLEXOS2DuckDB(output_path=FIXTURE_DB)


@pytest.fixture
def open_client(
    client: plexos2duckdb.PLEXOS2DuckDB,
) -> plexos2duckdb.PLEXOS2DuckDB:
    with client as db:
        yield db


def _visible_attrs(accessor: plexos2duckdb.ReportAccessor) -> list[str]:
    return sorted(name for name in dir(accessor) if not name.startswith("_"))


def test_report_returns_accessor(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    assert isinstance(open_client.report, plexos2duckdb.ReportAccessor)


def test_report_top_level_matches_schema(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    view_names = [
        row[0]
        for row in open_client.connection.execute(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = 'report'"
        ).fetchall()
    ]
    expected_phases = sorted({name.split("__")[0] for name in view_names})
    assert _visible_attrs(open_client.report) == expected_phases


def test_report_intermediate_navigation(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    lt = open_client.report.LT
    assert isinstance(lt, plexos2duckdb.ReportAccessor)
    assert _visible_attrs(lt) == ["Interval", "Year"]

    interval = lt.Interval
    assert isinstance(interval, plexos2duckdb.ReportAccessor)
    assert "Generators" in _visible_attrs(interval)

    generators = interval.Generators
    assert isinstance(generators, plexos2duckdb.ReportAccessor)
    assert "Generation" in _visible_attrs(generators)


def test_report_leaf_returns_duckdb_relation(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    relation = open_client.report.LT.Interval.Generators.Generation
    assert isinstance(relation, duckdb.DuckDBPyRelation)


def test_report_leaf_relation_has_expected_columns(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    relation = open_client.report.LT.Interval.Generators.Generation
    columns = list(relation.columns)
    # Columns are defined by ``create_report_views`` in ``src/lib.rs``.
    for expected in ("band", "sample_name", "name", "category", "value", "property", "unit"):
        assert expected in columns, columns


def test_report_leaf_matches_direct_sql(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    via_accessor = open_client.report.LT.Interval.Generators.Generation.fetchall()
    via_sql = open_client.connection.execute(
        'SELECT * FROM report."LT__Interval__Generators__Generation"'
    ).fetchall()
    assert via_accessor == via_sql
    assert via_accessor  # sanity: fixture is not empty


def test_report_dir_supports_tab_completion(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    entries = dir(open_client.report.LT.Interval)
    # Segments are surfaced so IPython/Jupyter can offer them for completion.
    assert "Generators" in entries
    assert "Batteries" in entries
    # Public helper on the class still visible (sanity check on ``dir``).
    assert "__class__" in entries


def test_report_missing_attribute_raises(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    with pytest.raises(AttributeError) as excinfo:
        open_client.report.LT.Interval.Generators.DoesNotExist  # noqa: B018
    message = str(excinfo.value)
    assert "report.LT.Interval.Generators" in message
    assert "'DoesNotExist'" in message
    assert "Generation" in message  # lists available siblings


def test_report_dunder_attribute_does_not_hit_tree(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    # Guards ``__getstate__``, ``__wrapped__``, IPython autoreload probes, etc.
    with pytest.raises(AttributeError):
        open_client.report.__wrapped__  # noqa: B018


def test_report_requires_open_connection(
    client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    with pytest.raises(plexos2duckdb.PLEXOS2DuckDBError):
        client.report  # noqa: B018


def test_report_repr_shows_path_and_keys(
    open_client: plexos2duckdb.PLEXOS2DuckDB,
) -> None:
    text = repr(open_client.report.LT.Interval)
    assert text.startswith("ReportAccessor(")
    assert "report.LT.Interval" in text
    assert "Generators" in text


def test_build_report_tree_skips_malformed_names() -> None:
    tree = api_module._build_report_tree(
        [
            "LT__Interval__Generators__Generation",
            "not_four_parts",
            "A__B__C",  # only 3 parts
            "A__B__C__D__E",  # 5 parts
            "ST__Year__Regions__Price",
        ]
    )
    assert tree == {
        "LT": {
            "Interval": {
                "Generators": {"Generation": "LT__Interval__Generators__Generation"}
            }
        },
        "ST": {"Year": {"Regions": {"Price": "ST__Year__Regions__Price"}}},
    }


def test_build_report_tree_empty_input() -> None:
    assert api_module._build_report_tree([]) == {}
