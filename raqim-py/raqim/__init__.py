from .client import RaqimClient

# Support both top-level site-packages module and package-local extension
try:
    from raqim_core import RaqimCryptoCore
except ImportError:
    try:
        from .raqim_core import RaqimCryptoCore
    except ImportError as e:
        raise ImportError(
            f"Failed to load compiled PyO3 native extension 'raqim_core': {e}. "
            "Ensure you ran 'maturin develop --release' inside your virtualenv."
        ) from e

__all__ = ["RaqimClient", "RaqimCryptoCore"]