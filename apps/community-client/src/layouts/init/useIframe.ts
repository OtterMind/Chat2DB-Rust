import { useGlobalStore } from "@/store/global";
import { useLayoutEffect } from "react";
import { IframeType } from "@/constants";
import useIframeMessage from "./useIframeMessage";
import { primaryColorsScales, ThemeAppearance } from "@chat2db/ui";
import { isEmbedIframePage } from "@/utils/iframe";
import { getLanguageType } from "@/utils";

/** Initialize iframe */
const useIframe = () => {
  const { setAppearance, setPrimaryColor, setLanguage, setIsEmbedIframe } =
  useGlobalStore((state) => {
    return {
      setAppearance: state.setAppearance,
      setPrimaryColor: state.setPrimaryColor,
      setLanguage: state.setLanguage,
      setIsEmbedIframe: state.setIsEmbedIframe,
    };
  });

  useIframeMessage();

  useLayoutEffect(() => { 
    // Get the query parameters of the current page
    const urlParams = new URLSearchParams(window.location.search);
  
    // Check if there is iframe=true parameter
    const iframe = urlParams.get('iframe');
    // If the iframe parameter does not exist, no initialization is performed
    if(!isEmbedIframePage()) {
      return;
    }
    const language = urlParams.get('language');
    const theme = urlParams.get('theme');
    const primaryColor = urlParams.get('primaryColor');
  
    if (iframe) {
      setIsEmbedIframe(iframe as IframeType);
    } else {
      setIsEmbedIframe(null);
    }

    if (language) {
      setLanguage(getLanguageType(language));
    }

    if (primaryColor) {
      setPrimaryColor(primaryColorsScales[primaryColor] || primaryColorsScales['purple']);
    }
  
    if (theme) {
      if (theme === 'light') {
        setAppearance(ThemeAppearance.Light);
      } else if (theme === 'dark_dimmed') {
        setAppearance(ThemeAppearance.DarkDimmed);
      } else {
        setAppearance(ThemeAppearance.Dark);
      }
    }
  }, [])
};

export default useIframe;
