"""Secret references keep credentials outside agent configuration files."""
from __future__ import annotations
import re
from dataclasses import dataclass
from typing import Mapping, Protocol

class SecretError(RuntimeError):
    """Raised when a required secret is not available."""
@dataclass(frozen=True, slots=True)
class SecretRef:
    name: str
    def __post_init__(self) -> None:
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", self.name):
            raise ValueError("secret names must be shell-safe identifiers")
    @property
    def token(self) -> str:
        return f"${{SECRET:{self.name}}}"
class SecretProvider(Protocol):
    def get(self, name: str) -> str: ...
@dataclass(frozen=True, slots=True)
class StaticSecretProvider:
    values: Mapping[str, str]
    def get(self, name: str) -> str:
        value = self.values.get(name)
        if not isinstance(value, str) or not value:
            raise SecretError(f"secret is unavailable: {name}")
        return value
_SECRET_PATTERN = re.compile(r"\$\{SECRET:([A-Za-z_][A-Za-z0-9_]*)\}")
def resolve_secret_refs(value: str, provider: SecretProvider) -> str:
    return _SECRET_PATTERN.sub(lambda m: provider.get(m.group(1)), value)
