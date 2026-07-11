class Service:
    def run(self):
        # `_live_helper` is accessed here; `_dead_helper` is never accessed.
        return self._live_helper()

    def _live_helper(self):
        return 1

    def _dead_helper(self):
        return 2

    def __init__(self):
        self.x = 0
