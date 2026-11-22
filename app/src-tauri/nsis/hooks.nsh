; Haru NSIS installer hooks (Tauri v2, included before the language files).
;
; Adds an optional "also delete the downloaded speech model" step to the
; uninstaller. The model lives in speech.cpp's cache, which is SHARED with other
; speech.cpp tools, so only Haru's own model folder is ever removed.
;
; Command line flag for scripted/silent uninstalls:  /DELETEMODEL

; Computes the speech.cpp models directory into OUT (mirrors paths.rs
; `models_dir_with`): %SPEECH_MODEL_DIR% if set, non-empty and absolute;
; "" if it is set but relative (the app refuses to run then: do nothing);
; otherwise $LOCALAPPDATA\speech.cpp\models.
; Absolute = drive form (X:\ or X:/) or UNC (\). Clobbers $R8 and $R9 and OUT.
!macro HARU_MODELS_DIR OUT
  SetShellVarContext current
  ReadEnvStr $R9 "SPEECH_MODEL_DIR"
  ${If} $R9 == ""
    StrCpy ${OUT} "$LOCALAPPDATA\speech.cpp\models"
  ${Else}
    StrCpy ${OUT} ""
    StrCpy $R8 $R9 2
    ${If} $R8 == "\\"
      StrCpy ${OUT} $R9
    ${Else}
      StrCpy $R8 $R9 1 1
      ${If} $R8 == ":"
        StrCpy $R8 $R9 1 2
        ${If} $R8 == "\"
        ${OrIf} $R8 == "/"
          StrCpy ${OUT} $R9
        ${EndIf}
      ${EndIf}
    ${EndIf}
    ; Drop one trailing slash for tidy paths
    ${If} ${OUT} != ""
      StrCpy $R8 ${OUT} 1 -1
      ${If} $R8 == "\"
      ${OrIf} $R8 == "/"
        StrCpy ${OUT} ${OUT} -1
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Push $0
    Push $1
    Push $R8
    Push $R9

    ; $1 = 1 when the model should be deleted
    StrCpy $1 0
    ; The error flag may still be set by earlier uninstall steps (e.g. a failed
    ; Delete/RMDir); ${Errors} below must only reflect the GetOptions result.
    ClearErrors
    ${GetOptions} $CMDLINE "/DELETEMODEL" $0
    ${IfNot} ${Errors}
      ; Explicit flag wins, also in silent/passive mode
      StrCpy $1 1
    ${ElseIfNot} ${Silent}
      ${If} $PassiveMode <> 1
        ${If} $LANGUAGE == 1031
          MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Auch das heruntergeladene Sprachmodell von Haru löschen (etwa 6,6 GB)?$\r$\n$\r$\nWenn du es behältst, muss es bei einer neuen Installation nicht erneut heruntergeladen werden." /SD IDNO IDNO haru_keep_model
        ${Else}
          MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also delete Haru's downloaded speech model (about 6.6 GB)?$\r$\n$\r$\nIf you keep it, a new installation will not need to download it again." /SD IDNO IDNO haru_keep_model
        ${EndIf}
        StrCpy $1 1
        haru_keep_model:
      ${EndIf}
    ${EndIf}

    ${If} $1 == 1
      !insertmacro HARU_MODELS_DIR $0
      ${If} $0 != ""
        ; Only Haru's own model folder, never the shared cache itself
        RMDir /r "$0\sakasegawa--Haru-4B-GGUF"
        ; Default location only: tidy up empty parents (non-recursive)
        ReadEnvStr $R9 "SPEECH_MODEL_DIR"
        ${If} $R9 == ""
          RMDir "$LOCALAPPDATA\speech.cpp\models"
          RMDir "$LOCALAPPDATA\speech.cpp"
        ${EndIf}
      ${EndIf}
    ${EndIf}

    Pop $R9
    Pop $R8
    Pop $1
    Pop $0
  ${EndIf}
!macroend
