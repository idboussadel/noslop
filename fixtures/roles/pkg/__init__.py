# Package init that re-exports its surface. Live because pkg.sub.mod is reachable;
# the `from` import is a re-export, not an unused-import.
from .sub.mod import thing

__all__ = ["thing"]
