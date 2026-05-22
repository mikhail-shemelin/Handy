import assert from "node:assert/strict";
import { describe, test } from "node:test";
import type { AppSettings } from "../src/bindings.ts";
import {
  DEFAULT_OPENAI_TRANSCRIPTION_MODEL,
  getOpenAiTranscriptionAvailability,
  getOpenAiTranscriptionModel,
  hasOpenAiTranscriptionApiKey,
  isOpenAiTranscriptionActive,
  isValidOpenAiTranscriptionEndpoint,
  OPENAI_TRANSCRIPTION_PROVIDER_ID,
} from "../src/lib/transcriptionRoute.ts";

const settings = (overrides: Partial<AppSettings>): AppSettings =>
  ({
    bindings: {},
    push_to_talk: true,
    audio_feedback: false,
    external_script_path: null,
    transcription_provider: "local",
    openai_transcription_enabled: false,
    openai_transcription_base_url: "https://api.openai.com/v1",
    openai_transcription_model: DEFAULT_OPENAI_TRANSCRIPTION_MODEL,
    openai_transcription_api_keys: {
      [OPENAI_TRANSCRIPTION_PROVIDER_ID]: "",
    },
    ...overrides,
  }) as AppSettings;

describe("transcription route helpers", () => {
  test("keeps OpenAI unavailable until explicitly enabled", () => {
    const availability = getOpenAiTranscriptionAvailability(settings({}));

    assert.equal(availability.enabled, false);
    assert.equal(availability.configured, false);
    assert.equal(availability.missingReasonKey, "modelSelector.openaiDisabled");
  });

  test("reports missing OpenAI API key when enabled but unconfigured", () => {
    const availability = getOpenAiTranscriptionAvailability(
      settings({ openai_transcription_enabled: true }),
    );

    assert.equal(availability.enabled, true);
    assert.equal(availability.configured, false);
    assert.equal(
      availability.missingReasonKey,
      "modelSelector.openaiMissingApiKey",
    );
  });

  test("treats a redacted saved API key as configured", () => {
    const appSettings = settings({
      openai_transcription_enabled: true,
      openai_transcription_api_keys: {
        [OPENAI_TRANSCRIPTION_PROVIDER_ID]: "[REDACTED]",
      },
    });

    assert.equal(hasOpenAiTranscriptionApiKey(appSettings), true);
    assert.equal(
      getOpenAiTranscriptionAvailability(appSettings).configured,
      true,
    );
  });

  test("rejects invalid OpenAI transcription endpoints", () => {
    const appSettings = settings({
      openai_transcription_enabled: true,
      openai_transcription_api_keys: {
        [OPENAI_TRANSCRIPTION_PROVIDER_ID]: "sk-test",
      },
      openai_transcription_base_url: "http://example.com",
    });

    assert.equal(
      isValidOpenAiTranscriptionEndpoint("https://example.com"),
      true,
    );
    assert.equal(
      isValidOpenAiTranscriptionEndpoint("http://localhost:8080"),
      true,
    );
    assert.equal(
      isValidOpenAiTranscriptionEndpoint("http://127.0.0.1:8080"),
      true,
    );
    assert.equal(
      isValidOpenAiTranscriptionEndpoint("http://127.0.0.2:8080"),
      true,
    );
    assert.equal(isValidOpenAiTranscriptionEndpoint("http://[::1]:8080"), true);
    assert.equal(
      isValidOpenAiTranscriptionEndpoint("http://example.com"),
      false,
    );
    assert.equal(
      isValidOpenAiTranscriptionEndpoint("http://api.localhost"),
      false,
    );
    assert.equal(
      isValidOpenAiTranscriptionEndpoint("https://example.com?bad=true"),
      false,
    );
    assert.equal(
      getOpenAiTranscriptionAvailability(appSettings).missingReasonKey,
      "modelSelector.openaiMissingEndpoint",
    );
    assert.equal(
      getOpenAiTranscriptionAvailability(appSettings).configured,
      false,
    );
  });

  test("uses the configured OpenAI model and falls back to the default label", () => {
    assert.equal(
      getOpenAiTranscriptionModel(
        settings({ openai_transcription_model: "whisper-1" }),
      ),
      "whisper-1",
    );
    assert.equal(
      getOpenAiTranscriptionModel(settings({ openai_transcription_model: "" })),
      DEFAULT_OPENAI_TRANSCRIPTION_MODEL,
    );
  });

  test("active route follows the provider setting independently of local model selection", () => {
    assert.equal(isOpenAiTranscriptionActive(settings({})), false);
    assert.equal(
      isOpenAiTranscriptionActive(
        settings({
          transcription_provider: "openai",
          openai_transcription_enabled: false,
        }),
      ),
      false,
    );
    assert.equal(
      isOpenAiTranscriptionActive(
        settings({
          transcription_provider: "openai",
          openai_transcription_enabled: true,
        }),
      ),
      true,
    );
  });
});
