import os

class Store:
    """Keeps values."""

    def get(self, key: str, default=None) -> str:
        return os.environ.get(key, default)  # fallback
