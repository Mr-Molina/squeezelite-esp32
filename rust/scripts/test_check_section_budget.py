"""Tests for check_section_budget.py that need no firmware ELF."""

import contextlib
import io
import unittest

import check_section_budget as budget

APP = "_RNvNvNtCslGOYIn9Eogu_16louder_esp32_app9xtensa_hw"


class HeapSymbolTests(unittest.TestCase):
    def test_matches_the_two_heap_statics(self):
        self.assertTrue(budget.is_heap_symbol(APP + "8bring_up4HEAP"))
        self.assertTrue(
            budget.is_heap_symbol(
                "_RNvNtCslGOYIn9Eogu_16louder_esp32_app9xtensa_hw14STATIC_BT_HEAP"
            )
        )

    def test_ignores_other_heap_named_symbols(self):
        self.assertFalse(budget.is_heap_symbol("_RNvCs5wyo8MYIC54_9esp_alloc4HEAP"))
        self.assertFalse(
            budget.is_heap_symbol(APP + "16http_serve_phase17INT_HEAP_MIN_FREE")
        )
        self.assertFalse(budget.is_heap_symbol("get_free_internal_heap_size"))


class HardLimitTests(unittest.TestCase):
    def run_main(self, *args):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = budget.main(["no-such-file.elf", *args])
        return code, err.getvalue()

    def test_defaults_are_inside_the_hard_limits(self):
        code, err = self.run_main()
        # The limits are accepted; only the missing file is reported.
        self.assertEqual(code, 2)
        self.assertNotIn("hard limit", err)

    def test_refuses_limits_beyond_invariant_48(self):
        for flag, value in (
            ("--max-iram", "131073"),
            ("--max-dram2", "98769"),
            ("--min-stack", "27999"),
            ("--min-heap", "65535"),
        ):
            code, err = self.run_main(flag, value)
            self.assertEqual(code, 2)
            self.assertIn("hard limit", err)
            self.assertIn(flag, err)


if __name__ == "__main__":
    unittest.main()
