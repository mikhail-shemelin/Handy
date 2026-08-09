import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import {
  DEFAULT_OPENAI_TRANSCRIPTION_MODEL,
  OPENAI_TRANSCRIPTION_MODELS,
} from "@/lib/transcriptionRoute";
import {
  SettingContainer,
  SettingsGroup,
  Textarea,
  ToggleSwitch,
} from "@/components/ui";
import { ApiKeyField } from "./PostProcessingSettingsApi/ApiKeyField";
import { BaseUrlField } from "./PostProcessingSettingsApi/BaseUrlField";
import { ModelSelect } from "./PostProcessingSettingsApi/ModelSelect";
import type { ModelOption } from "./PostProcessingSettingsApi/types";

const OPENAI_PROVIDER_ID = "openai";

const OPENAI_MODELS: ModelOption[] = OPENAI_TRANSCRIPTION_MODELS.map(
  (value) => ({ value, label: value }),
);

const supportsOpenAiChunking = (model: string) =>
  model === "gpt-transcribe" ||
  model === "gpt-4o-transcribe" ||
  model === "gpt-4o-mini-transcribe" ||
  model === "gpt-4o-transcribe-diarize";

export const TranscriptionProviderSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, refreshSettings, isUpdating } =
    useSettings();
  const [isApiKeyUpdating, setIsApiKeyUpdating] = useState(false);
  const [isOpenAiToggleUpdating, setIsOpenAiToggleUpdating] = useState(false);
  const [promptDraft, setPromptDraft] = useState("");

  const provider = settings?.transcription_provider ?? "local";
  const isOpenAiActive = provider === OPENAI_PROVIDER_ID;
  const openAiEnabled = settings?.openai_transcription_enabled ?? false;
  const apiKey =
    settings?.openai_transcription_api_keys?.[OPENAI_PROVIDER_ID] ?? "";
  const baseUrl = settings?.openai_transcription_base_url ?? "";
  const model =
    settings?.openai_transcription_model ?? DEFAULT_OPENAI_TRANSCRIPTION_MODEL;
  const prompt = settings?.openai_transcription_prompt ?? "";
  const chunkingEnabled =
    settings?.openai_transcription_chunking_enabled ?? false;
  const translateToEnglish = settings?.translate_to_english ?? false;
  const supportsChunking = supportsOpenAiChunking(model) && !translateToEnglish;
  const effectiveChunkingEnabled = supportsChunking && chunkingEnabled;
  useEffect(() => {
    setPromptDraft(prompt);
  }, [prompt]);
  const modelOptions = useMemo(() => {
    if (!model || OPENAI_MODELS.some((option) => option.value === model)) {
      return OPENAI_MODELS;
    }

    return [...OPENAI_MODELS, { value: model, label: model }];
  }, [model]);

  const handleOpenAiEnabledChange = useCallback(
    async (enabled: boolean) => {
      setIsOpenAiToggleUpdating(true);
      try {
        const result =
          await commands.changeOpenaiTranscriptionEnabledSetting(enabled);
        if (result.status === "error") {
          throw new Error(String(result.error));
        }
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update OpenAI transcription toggle:", error);
        await refreshSettings();
        if (!enabled && isOpenAiActive) {
          toast.error(
            t("settings.transcription.openai.enabled.disableBlocked"),
          );
        }
      } finally {
        setIsOpenAiToggleUpdating(false);
      }
    },
    [isOpenAiActive, refreshSettings, t],
  );

  const handleBaseUrlChange = useCallback(
    async (value: string) => {
      const trimmed = value.trim().replace(/\/+$/, "");
      if (trimmed !== baseUrl) {
        await updateSetting("openai_transcription_base_url", trimmed);
        await refreshSettings();
      }
    },
    [baseUrl, refreshSettings, updateSetting],
  );

  const handleApiKeyChange = useCallback(
    async (value: string) => {
      const trimmed = value.trim();
      if (trimmed === apiKey) return;

      setIsApiKeyUpdating(true);
      try {
        const result =
          await commands.changeOpenaiTranscriptionApiKeySetting(trimmed);
        if (result.status === "error") {
          throw new Error(String(result.error));
        }
        await refreshSettings();
      } catch (error) {
        console.error("Failed to update OpenAI transcription API key:", error);
        await refreshSettings();
      } finally {
        setIsApiKeyUpdating(false);
      }
    },
    [apiKey, refreshSettings],
  );

  const handleModelSelect = useCallback(
    async (value: string) => {
      await updateSetting("openai_transcription_model", value.trim());
      await refreshSettings();
    },
    [refreshSettings, updateSetting],
  );

  const handleModelBlur = useCallback(() => {}, []);

  const handlePromptBlur = useCallback(async () => {
    const value = promptDraft.trim();
    if (value !== prompt) {
      await updateSetting("openai_transcription_prompt", value);
      await refreshSettings();
    }
  }, [prompt, promptDraft, refreshSettings, updateSetting]);

  return (
    <SettingsGroup title={t("settings.transcription.title")}>
      <ToggleSwitch
        checked={openAiEnabled}
        onChange={(enabled) => void handleOpenAiEnabledChange(enabled)}
        isUpdating={
          isOpenAiToggleUpdating || isUpdating("openai_transcription_enabled")
        }
        label={t("settings.transcription.openai.enabled.title")}
        description={t("settings.transcription.openai.enabled.description")}
        descriptionMode="tooltip"
        grouped={true}
      />

      {openAiEnabled && (
        <>
          <SettingContainer
            title={t("settings.transcription.openai.endpoint.title")}
            description={t(
              "settings.transcription.openai.endpoint.description",
            )}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <BaseUrlField
              value={baseUrl}
              onBlur={handleBaseUrlChange}
              placeholder={t(
                "settings.transcription.openai.endpoint.placeholder",
              )}
              disabled={isUpdating("openai_transcription_base_url")}
              className="min-w-[380px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.transcription.openai.apiKey.title")}
            description={t("settings.transcription.openai.apiKey.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <ApiKeyField
              value={apiKey}
              onBlur={handleApiKeyChange}
              placeholder={t(
                "settings.transcription.openai.apiKey.placeholder",
              )}
              disabled={isApiKeyUpdating}
              className="min-w-[320px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.transcription.openai.model.title")}
            description={t("settings.transcription.openai.model.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <ModelSelect
              value={model}
              options={modelOptions}
              onSelect={handleModelSelect}
              onCreate={handleModelSelect}
              onBlur={handleModelBlur}
              disabled={isUpdating("openai_transcription_model")}
              className="min-w-[380px]"
            />
          </SettingContainer>

          <SettingContainer
            title={t("settings.transcription.openai.prompt.title")}
            description={t("settings.transcription.openai.prompt.description")}
            descriptionMode="tooltip"
            layout="horizontal"
            grouped={true}
          >
            <Textarea
              value={promptDraft}
              onChange={(event) => setPromptDraft(event.target.value)}
              onBlur={handlePromptBlur}
              placeholder={t(
                "settings.transcription.openai.prompt.placeholder",
              )}
              disabled={isUpdating("openai_transcription_prompt")}
              variant="compact"
              className="min-w-[380px]"
            />
          </SettingContainer>

          <ToggleSwitch
            checked={effectiveChunkingEnabled}
            onChange={(enabled) =>
              void updateSetting(
                "openai_transcription_chunking_enabled",
                enabled,
              )
            }
            isUpdating={isUpdating("openai_transcription_chunking_enabled")}
            disabled={!supportsChunking}
            label={t("settings.transcription.openai.chunking.title")}
            description={t(
              "settings.transcription.openai.chunking.description",
            )}
            descriptionMode="tooltip"
            grouped={true}
          />
        </>
      )}
    </SettingsGroup>
  );
};
