from app.cycle_a import alpha


def beta():
    # Reference alpha lazily to form a runtime-relevant import cycle.
    return 41 if alpha else 0
