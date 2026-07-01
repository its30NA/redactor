"""LLM detector — exercised with in-memory fake backends (no model, no network)."""

from __future__ import annotations

from redactor.detectors.llm import LLMSecretDetector, _parse_items
from redactor.llm.backend import LLMBackend, LLMUnavailable
from redactor.pipeline import Pipeline


class FakeBackend:
    """Returns a canned completion; records the prompt it was given."""

    def __init__(self, reply: str) -> None:
        self.reply = reply
        self.prompt: str | None = None

    def complete(self, prompt: str) -> str:
        self.prompt = prompt
        return self.reply


class DownBackend:
    def complete(self, prompt: str) -> str:
        raise LLMUnavailable("connection refused")


def test_fake_backend_satisfies_protocol() -> None:
    assert isinstance(FakeBackend("[]"), LLMBackend)


def test_llm_detects_verbatim_span() -> None:
    text = "the launch code is ZULU-ALPHA-9 for the mission"
    backend = FakeBackend('[{"text": "ZULU-ALPHA-9", "type": "Launch Code"}]')
    pipe = Pipeline(detectors=[LLMSecretDetector(backend)])
    result = pipe.sanitize(text)
    assert result.text == "the launch code is [REDACTED: Launch Code] for the mission"


def test_llm_ignores_non_verbatim_hallucination() -> None:
    # Model returns something not present in the text -> must be ignored.
    text = "nothing sensitive here"
    backend = FakeBackend('[{"text": "sk-totally-made-up-key", "type": "API Key"}]')
    result = Pipeline(detectors=[LLMSecretDetector(backend)]).sanitize(text)
    assert result.matches == []
    assert result.text == text


def test_llm_fails_open_when_backend_down() -> None:
    errors: list[Exception] = []
    detector = LLMSecretDetector(DownBackend(), on_error=errors.append)
    result = Pipeline(detectors=[detector]).sanitize("some text with a token=abc123")
    assert result.matches == []  # no crash; deterministic layer would still apply
    assert len(errors) == 1
    assert isinstance(errors[0], LLMUnavailable)


def test_llm_handles_chatty_non_json_reply() -> None:
    text = "password is swordfish123"
    reply = 'Sure! Here you go:\n[{"text": "swordfish123", "type": "Password"}]\nHope that helps.'
    result = Pipeline(detectors=[LLMSecretDetector(backend=FakeBackend(reply))]).sanitize(text)
    assert result.text == "password is [REDACTED: Password]"


def test_llm_off_by_default_in_pipeline() -> None:
    # A plain Pipeline never includes the LLM detector.
    assert all(d.name != "llm_secret" for d in Pipeline().detectors)


def test_parse_items_tolerates_garbage() -> None:
    assert _parse_items("not json at all") == []
    assert _parse_items("[1, 2, 3]") == []  # non-dict elements dropped
    assert _parse_items('[{"text": "x"}]') == [{"text": "x"}]
