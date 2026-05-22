import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { useModelStore } from "../../../stores/modelStore";
import { useSettings } from "../../../hooks/useSettings";
import type { ModelInfo } from "@/bindings";

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models } = useModelStore();
  const { settings } = useSettings();

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);
  const isOpenAiActive = settings?.transcription_provider === "openai";

  const supportsLanguageSelection =
    currentModelInfo?.supports_language_selection ?? false;
  const supportsTranslation =
    isOpenAiActive || (currentModelInfo?.supports_translation ?? false);
  const showsLanguageSelector = supportsLanguageSelection || isOpenAiActive;
  const hasAnySettings = showsLanguageSelector || supportsTranslation;
  const settingsModelName = isOpenAiActive
    ? settings?.openai_transcription_model ||
      t("settings.transcription.provider.options.openai")
    : currentModelInfo?.name;

  // Don't render anything if no model is selected or no settings available
  if (
    (!isOpenAiActive && (!currentModel || !currentModelInfo)) ||
    !hasAnySettings
  ) {
    return null;
  }

  return (
    <SettingsGroup
      title={t("settings.modelSettings.title", {
        model: settingsModelName,
      })}
    >
      {showsLanguageSelector && (
        <LanguageSelector
          descriptionMode="tooltip"
          grouped={true}
          supportedLanguages={
            isOpenAiActive ? undefined : currentModelInfo?.supported_languages
          }
        />
      )}
      {supportsTranslation && (
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
      )}
    </SettingsGroup>
  );
};
