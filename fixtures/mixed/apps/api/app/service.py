import requests  # declared dep, used → not unused-dependency

from app.cycle_a import alpha


def _scale(x: int) -> int:
    return x * 2


def compute(x: int) -> int:
    _ = requests
    return _scale(x) + alpha()
