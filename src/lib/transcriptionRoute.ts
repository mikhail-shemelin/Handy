import type { AppSettings } from "@/bindings";

export const OPENAI_TRANSCRIPTION_ROUTE_ID = "__openai_transcription__";
export const OPENAI_TRANSCRIPTION_PROVIDER_ID = "openai";
export const DEFAULT_OPENAI_TRANSCRIPTION_MODEL = "gpt-4o-transcribe";

const REDACTED_SECRET_PLACEHOLDER = "[REDACTED]";

export type OpenAiTranscriptionAvailability = {
  enabled: boolean;
  configured: boolean;
  missingReasonKey: string | null;
};

export const getOpenAiTranscriptionModel = (
  settings: AppSettings | null,
): string => {
  return (
    settings?.openai_transcription_model?.trim() ||
    DEFAULT_OPENAI_TRANSCRIPTION_MODEL
  );
};

export const hasOpenAiTranscriptionApiKey = (
  settings: AppSettings | null,
): boolean => {
  const apiKey =
    settings?.openai_transcription_api_keys?.[
      OPENAI_TRANSCRIPTION_PROVIDER_ID
    ] ?? "";

  const trimmed = apiKey.trim();
  return trimmed.length > 0 || trimmed === REDACTED_SECRET_PLACEHOLDER;
};

export const isValidOpenAiTranscriptionEndpoint = (
  endpoint: string | null | undefined,
): boolean => {
  const trimmed = endpoint?.trim().replace(/\/+$/, "") ?? "";
  if (!trimmed) return false;

  try {
    const url = new URL(trimmed);
    if (url.search || url.hash) return false;

    if (url.protocol === "https:") return true;
    if (url.protocol !== "http:") return false;

    const hostname = url.hostname
      .toLowerCase()
      .replace(/^\[/, "")
      .replace(/\]$/, "");
    return hostname === "localhost" || isLoopbackIpAddress(hostname);
  } catch {
    return false;
  }
};

const isLoopbackIpAddress = (hostname: string): boolean => {
  if (hostname === "::1") return true;

  const octets = hostname.split(".");
  if (octets.length !== 4) return false;

  const values = octets.map((octet) => Number(octet));
  return (
    values.every(
      (value) => Number.isInteger(value) && value >= 0 && value <= 255,
    ) && values[0] === 127
  );
};

export const getOpenAiTranscriptionAvailability = (
  settings: AppSettings | null,
): OpenAiTranscriptionAvailability => {
  const enabled = settings?.openai_transcription_enabled ?? false;

  if (!enabled) {
    return {
      enabled,
      configured: false,
      missingReasonKey: "modelSelector.openaiDisabled",
    };
  }

  if (!hasOpenAiTranscriptionApiKey(settings)) {
    return {
      enabled,
      configured: false,
      missingReasonKey: "modelSelector.openaiMissingApiKey",
    };
  }

  if (
    !isValidOpenAiTranscriptionEndpoint(settings?.openai_transcription_base_url)
  ) {
    return {
      enabled,
      configured: false,
      missingReasonKey: "modelSelector.openaiMissingEndpoint",
    };
  }

  if (!settings?.openai_transcription_model?.trim()) {
    return {
      enabled,
      configured: false,
      missingReasonKey: "modelSelector.openaiMissingModel",
    };
  }

  return {
    enabled,
    configured: true,
    missingReasonKey: null,
  };
};

export const isOpenAiTranscriptionActive = (
  settings: AppSettings | null,
): boolean => {
  return (
    settings?.openai_transcription_enabled === true &&
    settings?.transcription_provider === OPENAI_TRANSCRIPTION_PROVIDER_ID
  );
};
