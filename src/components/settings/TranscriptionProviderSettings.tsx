import React, { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type TranscriptionProvider } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Dropdown, SettingContainer, SettingsGroup } from "@/components/ui";
import { ApiKeyField } from "./PostProcessingSettingsApi/ApiKeyField";
import { BaseUrlField } from "./PostProcessingSettingsApi/BaseUrlField";
import { ModelSelect } from "./PostProcessingSettingsApi/ModelSelect";
import type { ModelOption } from "./PostProcessingSettingsApi/types";

const OPENAI_PROVIDER_ID = "openai";

const OPENAI_MODELS: ModelOption[] = [
  { value: "gpt-4o-transcribe", label: "gpt-4o-transcribe" },
  { value: "gpt-4o-mini-transcribe", label: "gpt-4o-mini-transcribe" },
  { value: "whisper-1", label: "whisper-1" },
];

type TranscriptionProviderSettingsProps = {
  onOpenAiApiKeySaved?: () => void | Promise<void>;
};

export const TranscriptionProviderSettings: React.FC<
  TranscriptionProviderSettingsProps
> = ({ onOpenAiApiKeySaved }) => {
  const { t } = useTranslation();
  const { settings, updateSetting, refreshSettings, isUpdating } =
    useSettings();
  const [isApiKeyUpdating, setIsApiKeyUpdating] = useState(false);

  const provider = settings?.transcription_provider ?? "local";
  const isOpenAi = provider === OPENAI_PROVIDER_ID;
  const apiKey =
    settings?.openai_transcription_api_keys?.[OPENAI_PROVIDER_ID] ?? "";
  const baseUrl = settings?.openai_transcription_base_url ?? "";
  const model = settings?.openai_transcription_model ?? "gpt-4o-transcribe";
  const modelOptions = useMemo(() => {
    if (!model || OPENAI_MODELS.some((option) => option.value === model)) {
      return OPENAI_MODELS;
    }

    return [...OPENAI_MODELS, { value: model, label: model }];
  }, [model]);

  const providerOptions = [
    {
      value: "local",
      label: t("settings.transcription.provider.options.local"),
    },
    {
      value: OPENAI_PROVIDER_ID,
      label: t("settings.transcription.provider.options.openai"),
    },
  ];

  const handleProviderSelect = useCallback(
    (value: string) => {
      void updateSetting(
        "transcription_provider",
        value as TranscriptionProvider,
      );
    },
    [updateSetting],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      const trimmed = value.trim().replace(/\/+$/, "");
      if (trimmed !== baseUrl) {
        void updateSetting("openai_transcription_base_url", trimmed);
      }
    },
    [baseUrl, updateSetting],
  );

  const handleApiKeyChange = useCallback(
    async (value: string) => {
      const trimmed = value.trim();
      if (trimmed === apiKey) return;

      setIsApiKeyUpdating(true);
      try {
        if (provider === OPENAI_PROVIDER_ID) {
          const providerResult =
            await commands.changeTranscriptionProviderSetting(
              provider as TranscriptionProvider,
            );
          if (providerResult.status === "error") {
            throw new Error(String(providerResult.error));
          }
        }

        const result =
          await commands.changeOpenaiTranscriptionApiKeySetting(trimmed);
        if (result.status === "error") {
          throw new Error(String(result.error));
        }
        await refreshSettings();
        await onOpenAiApiKeySaved?.();
      } catch (error) {
        console.error("Failed to update OpenAI transcription API key:", error);
        await refreshSettings();
      } finally {
        setIsApiKeyUpdating(false);
      }
    },
    [apiKey, onOpenAiApiKeySaved, provider, refreshSettings],
  );

  const handleModelSelect = useCallback(
    (value: string) => {
      void updateSetting("openai_transcription_model", value.trim());
    },
    [updateSetting],
  );

  const handleModelBlur = useCallback(() => {}, []);

  return (
    <SettingsGroup title={t("settings.transcription.title")}>
      <SettingContainer
        title={t("settings.transcription.provider.title")}
        description={t("settings.transcription.provider.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <Dropdown
          options={providerOptions}
          selectedValue={provider}
          onSelect={handleProviderSelect}
          disabled={isUpdating("transcription_provider")}
        />
      </SettingContainer>

      {isOpenAi && (
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
        </>
      )}
    </SettingsGroup>
  );
};
