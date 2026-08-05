; Hangar.AI installer chrome.
;
; NSIS ships a wizard: a captioned window with a header strip, a Back/Next/Cancel
; row and a page per decision. This file replaces that with the shape a modern
; desktop app installs in — one small frameless window, the product mark, one
; button, a progress line, done — by hiding every stock control and drawing over
; the space they leave behind.
;
; What sits underneath is unchanged: NSIS still runs its pages in order and the
; sections still do the installing. Only the surface is ours.
;
; Included by installer.nsi (the real installer) and by scripts/preview.nsi (a
; throwaway one used to look at the result), so the two cannot drift.
;
; Must be included after MUI2.nsh, and before the first page.

!ifndef IABENCH_THEME_NSH
!define IABENCH_THEME_NSH

!include LogicLib.nsh
!include WinMessages.nsh
!include nsDialogs.nsh

; Window size in logical pixels. scripts/installer-art.ps1 draws backdrop.bmp at
; twice this, and every coordinate below is against it.
!define IB_WIN_W 520
!define IB_WIN_H 340

; Tokyo Night, the theme the app opens with. Kept in sync by hand with
; src/themes.ts and scripts/installer-art.ps1.
!define IB_TEXT "F0F3FC"
!define IB_TEXT_DIM "7C84A3"
!define IB_ACCENT "22C9DD"
!define IB_ON_ACCENT "0E1017"
; Win32 messages want COLORREF, which is 0x00BBGGRR — the reverse of the above.
!define IB_ACCENT_BGR 0xDDC922
!define IB_TRACK_BGR 0x3B2A27

; The backdrop bitmap covers the window, so any control that does not paint over
; it has to let it through. NSIS spells that as a background colour.
!define IB_CLEAR "transparent"

!define IB_GWL_STYLE -16
!define IB_WS_CAPTION 0x00C00000
!define IB_WS_SYSMENU 0x00080000
!define IB_WS_THICKFRAME 0x00040000
!define IB_WS_MINIMIZEBOX 0x00020000
!define IB_WS_MAXIMIZEBOX 0x00010000
!define IB_SWP_FRAMECHANGED 0x0020
!define IB_SWP_NOZORDER 0x0004
!define IB_SWP_NOMOVE 0x0002
!define IB_SWP_NOSIZE 0x0001
!define IB_HWND_BOTTOM 1
!define IB_DWMWA_CORNERS 33
!define IB_CORNERS_ROUND 2
!define IB_LR_LOADFROMFILE 0x0010
!define /ifndef PBM_SETBARCOLOR 0x0409
!define /ifndef PBM_SETBKCOLOR 0x2001

; Interior page controls, by dialog resource id.
!define IB_ID_PROGRESS 1004
!define IB_ID_STATUS 1006
!define IB_ID_LOG 1016
!define IB_ID_DETAILS 1027

Var IB_Dpi
Var IB_FontTitle
Var IB_FontBody
Var IB_FontButton
Var IB_FontClose
Var IB_Backdrop   ; the bitmap that matches this display, in $PLUGINSDIR
Var IB_Dialog     ; the dialog controls are added to; set by each page
Var IB_Close
Var IB_Wordmark
Var IB_Tagline
Var IB_Status
Var IB_Primary    ; the one button a page has
Var IB_Note       ; the one line of text under or above it

; Captured while this file is being included, so it points at the directory the
; art lives in no matter where the including script sits — Tauri generates the
; real installer.nsi into target/, far away from here.
!define IB_ASSET_DIR "${__FILEDIR__}"

; Unpacks the backdrops for IB_Backdrop to load. Call from .onInit.
!macro IB_ExtractAssets
  InitPluginsDir
  File "/oname=$PLUGINSDIR\backdrop.bmp" "${IB_ASSET_DIR}\backdrop.bmp"
  File "/oname=$PLUGINSDIR\backdrop@2x.bmp" "${IB_ASSET_DIR}\backdrop@2x.bmp"
!macroend

; Everything is authored at 96 DPI and scaled on the way out.
!macro IB_Scale var
  IntOp ${var} ${var} * $IB_Dpi
  IntOp ${var} ${var} / 96
!macroend

!macro IB_Font out px weight
  Push $0
  IntOp $0 ${px} * $IB_Dpi
  IntOp $0 $0 / 96
  IntOp $0 0 - $0
  System::Call 'gdi32::CreateFont(i $0, i 0, i 0, i 0, i ${weight}, i 0, i 0, i 0, \
    i 1, i 0, i 0, i 5, i 0, t "Segoe UI") p .s'
  Pop ${out}
  Pop $0
!macroend

; Rounds a control by clipping it, which is the only rounding Win32 offers
; without owner-drawing every pixel.
!macro IB_Round hwnd w h radius
  Push $0
  Push $1
  Push $2
  StrCpy $0 ${w}
  StrCpy $1 ${h}
  StrCpy $2 ${radius}
  !insertmacro IB_Scale $0
  !insertmacro IB_Scale $1
  !insertmacro IB_Scale $2
  IntOp $0 $0 + 1
  IntOp $1 $1 + 1
  System::Call 'gdi32::CreateRoundRectRgn(i 0, i 0, i $0, i $1, i $2, i $2) p .s'
  Pop $0
  System::Call 'user32::SetWindowRgn(p ${hwnd}, p $0, i 1)'
  Pop $2
  Pop $1
  Pop $0
!macroend

; ---------------------------------------------------------------------------
; The window
; ---------------------------------------------------------------------------

; NSIS re-shows the Back/Next/Cancel row as it moves between pages, so hiding a
; control is not enough on its own — it is also parked off the window.
!macro IB_HideCtl id
  GetDlgItem $3 $HWNDPARENT ${id}
  ${If} $3 <> 0
    ShowWindow $3 ${SW_HIDE}
    System::Call 'user32::SetWindowPos(p $3, p 0, i 0, i 9000, i 0, i 0, \
      i ${IB_SWP_NOZORDER}|${IB_SWP_NOSIZE})'
  ${EndIf}
!macroend

; Strips the wizard frame down to a bare panel and resizes it. Runs once, before
; any page is shown — from MUI_CUSTOMFUNCTION_GUIINIT when installing and from
; MUI_CUSTOMFUNCTION_UNGUIINIT when uninstalling. NSIS keeps the two programs
; strictly apart, so this is a macro with a function on each side of the fence
; rather than one function called twice.
!macro IB_InitWindowBody
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4

  System::Call 'user32::GetDpiForWindow(p $HWNDPARENT) i .r0'
  ${If} $0 < 96
    StrCpy $0 96
  ${EndIf}
  StrCpy $IB_Dpi $0

  !insertmacro IB_Font $IB_FontTitle 36 600
  !insertmacro IB_Font $IB_FontBody 15 400
  !insertmacro IB_Font $IB_FontButton 15 600
  !insertmacro IB_Font $IB_FontClose 20 400

  ${If} $IB_Dpi >= 144
    StrCpy $IB_Backdrop "$PLUGINSDIR\backdrop@2x.bmp"
  ${Else}
    StrCpy $IB_Backdrop "$PLUGINSDIR\backdrop.bmp"
  ${EndIf}

  ; Caption, resize grip and window buttons all go. WS_SYSMENU stays: it draws
  ; nothing without a caption, but it is what keeps Alt+F4 working, and with no
  ; title bar that is the only way out of a page that has no close button.
  System::Call 'user32::GetWindowLong(p $HWNDPARENT, i ${IB_GWL_STYLE}) i .r0'
  IntOp $1 ${IB_WS_CAPTION} | ${IB_WS_THICKFRAME}
  IntOp $1 $1 | ${IB_WS_MINIMIZEBOX}
  IntOp $1 $1 | ${IB_WS_MAXIMIZEBOX}
  IntOp $1 $1 ~
  IntOp $0 $0 & $1
  System::Call 'user32::SetWindowLong(p $HWNDPARENT, i ${IB_GWL_STYLE}, i $0)'

  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i ${IB_DWMWA_CORNERS}, \
    *i ${IB_CORNERS_ROUND}, i 4)'

  StrCpy $0 ${IB_WIN_W}
  StrCpy $1 ${IB_WIN_H}
  !insertmacro IB_Scale $0
  !insertmacro IB_Scale $1
  System::Call 'user32::GetSystemMetrics(i 0) i .r2'
  System::Call 'user32::GetSystemMetrics(i 1) i .r4'
  IntOp $2 $2 - $0
  IntOp $2 $2 / 2
  IntOp $4 $4 - $1
  IntOp $4 $4 / 2
  System::Call 'user32::SetWindowPos(p $HWNDPARENT, p 0, i $2, i $4, i $0, i $1, \
    i ${IB_SWP_FRAMECHANGED}|${IB_SWP_NOZORDER})'

  ; The wizard furniture: header block, separators, branding line, button row.
  !insertmacro IB_HideCtl 1034
  !insertmacro IB_HideCtl 1035
  !insertmacro IB_HideCtl 1037
  !insertmacro IB_HideCtl 1038
  !insertmacro IB_HideCtl 1039
  !insertmacro IB_HideCtl 1045
  !insertmacro IB_HideCtl 1028
  !insertmacro IB_HideCtl 1256
  !insertmacro IB_HideCtl 1
  !insertmacro IB_HideCtl 2
  !insertmacro IB_HideCtl 3

  ; The page area is what is left, so it becomes the whole window.
  GetDlgItem $3 $HWNDPARENT 1018
  ${If} $3 <> 0
    System::Call 'user32::SetWindowPos(p $3, p 0, i 0, i 0, i $0, i $1, \
      i ${IB_SWP_NOZORDER})'
  ${EndIf}

  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

Function IB_InitWindow
  !insertmacro IB_InitWindowBody
FunctionEnd

Function un.IB_InitWindow
  !insertmacro IB_InitWindowBody
FunctionEnd

; ---------------------------------------------------------------------------
; Controls
;
; All of these add to $IB_Dialog, which each page sets first. They take their
; geometry in logical pixels against the 520x340 window.
; ---------------------------------------------------------------------------

; Covers the page with the backdrop bitmap. Call before adding anything else:
; siblings created later sit above it.
!macro IB_Backdrop
  Push $0
  Push $1
  Push $2

  StrCpy $0 ${IB_WIN_W}
  StrCpy $1 ${IB_WIN_H}
  !insertmacro IB_Scale $0
  !insertmacro IB_Scale $1

  System::Call 'user32::CreateWindowEx(i 0, t "STATIC", t "", \
    i ${WS_CHILD}|${WS_VISIBLE}|${SS_BITMAP}, i 0, i 0, i $0, i $1, \
    p $IB_Dialog, i 0, i 0, i 0) p .s'
  Pop $2

  ; LoadImage resizes as it loads, which is the only way a static control shows
  ; a bitmap at anything other than its native size. It stretches by dropping and
  ; doubling rows, so $IB_Backdrop is whichever of the two sizes needs the least
  ; of it — and nothing with an edge to ruin is drawn into it either way.
  System::Call 'user32::LoadImage(i 0, t "$IB_Backdrop", i 0, \
    i $0, i $1, i ${IB_LR_LOADFROMFILE}) p .s'
  Pop $0
  SendMessage $2 ${STM_SETIMAGE} ${IMAGE_BITMAP} $0
  System::Call 'user32::SetWindowPos(p $2, p ${IB_HWND_BOTTOM}, i 0, i 0, i 0, i 0, \
    i ${IB_SWP_NOMOVE}|${IB_SWP_NOSIZE})'

  Pop $2
  Pop $1
  Pop $0
!macroend

!macro IB_MakeCtl out x y w h text style
  Push $0
  Push $1
  Push $2
  Push $3

  StrCpy $0 ${x}
  StrCpy $1 ${y}
  StrCpy $2 ${w}
  StrCpy $3 ${h}
  !insertmacro IB_Scale $0
  !insertmacro IB_Scale $1
  !insertmacro IB_Scale $2
  !insertmacro IB_Scale $3

  System::Call 'user32::CreateWindowEx(i 0, t "STATIC", t "${text}", \
    i ${style}, i $0, i $1, i $2, i $3, p $IB_Dialog, i 0, i 0, i 0) p .s'
  Pop ${out}

  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; A flat accent button. Real Win32 buttons are painted by the visual style and
; ignore colours entirely, so this is a static that behaves like one.
!macro IB_Button out x y w h text
  !insertmacro IB_MakeCtl ${out} ${x} ${y} ${w} ${h} "${text}" \
    "${WS_CHILD}|${WS_VISIBLE}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE}"
  SendMessage ${out} ${WM_SETFONT} $IB_FontButton 1
  SetCtlColors ${out} "${IB_ON_ACCENT}" "${IB_ACCENT}"
  !insertmacro IB_Round ${out} ${w} ${h} 8
!macroend

; A line of text over the backdrop. `dim` picks the muted colour.
!macro IB_Label out x y w h text dim
  !insertmacro IB_MakeCtl ${out} ${x} ${y} ${w} ${h} "${text}" \
    "${WS_CHILD}|${WS_VISIBLE}|${SS_CENTER}|${SS_CENTERIMAGE}|${SS_ENDELLIPSIS}"
  SendMessage ${out} ${WM_SETFONT} $IB_FontBody 1
  !if "${dim}" == "dim"
    SetCtlColors ${out} "${IB_TEXT_DIM}" ${IB_CLEAR}
  !else
    SetCtlColors ${out} "${IB_TEXT}" ${IB_CLEAR}
  !endif
!macroend

; Moves a control that already exists — one nsDialogs made, or one that came
; with a dialog resource — into this layout.
!macro IB_Place hwnd x y w h
  Push $0
  Push $1
  Push $2
  Push $3

  StrCpy $0 ${x}
  StrCpy $1 ${y}
  StrCpy $2 ${w}
  StrCpy $3 ${h}
  !insertmacro IB_Scale $0
  !insertmacro IB_Scale $1
  !insertmacro IB_Scale $2
  !insertmacro IB_Scale $3
  System::Call 'user32::SetWindowPos(p ${hwnd}, p 0, i $0, i $1, i $2, i $3, \
    i ${IB_SWP_NOZORDER})'

  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro IB_PlaceLabel hwnd x y w h
  !insertmacro IB_Place ${hwnd} ${x} ${y} ${w} ${h}
  SendMessage ${hwnd} ${WM_SETFONT} $IB_FontBody 1
  SetCtlColors ${hwnd} "${IB_TEXT}" ${IB_CLEAR}
!macroend

; Radio buttons and check boxes are drawn by the visual style, which paints
; their label in the system colour and ignores SetCtlColors. Dropping the theme
; costs the rounded glyph but hands the text back.
!macro IB_PlaceChoice hwnd x y w h
  !insertmacro IB_Place ${hwnd} ${x} ${y} ${w} ${h}
  System::Call 'uxtheme::SetWindowTheme(p ${hwnd}, w " ", w " ")'
  SendMessage ${hwnd} ${WM_SETFONT} $IB_FontBody 1
  SetCtlColors ${hwnd} "${IB_TEXT}" ${IB_CLEAR}
!macroend

; The product name and one line of copy, in live text rather than baked into the
; backdrop, so they are rendered by the system at the display's own resolution.
!macro IB_Masthead name tagline
  !insertmacro IB_MakeCtl $IB_Wordmark 40 134 440 52 "${name}" \
    "${WS_CHILD}|${WS_VISIBLE}|${SS_CENTER}|${SS_CENTERIMAGE}"
  SendMessage $IB_Wordmark ${WM_SETFONT} $IB_FontTitle 1
  SetCtlColors $IB_Wordmark "${IB_TEXT}" ${IB_CLEAR}

  !insertmacro IB_Label $IB_Tagline 40 192 440 20 "${tagline}" dim
!macroend

; Without a caption there is no close button, so every page draws its own.
!macro IB_CloseButton onclick
  !insertmacro IB_MakeCtl $IB_Close 476 12 32 32 "×" \
    "${WS_CHILD}|${WS_VISIBLE}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE}"
  SendMessage $IB_Close ${WM_SETFONT} $IB_FontClose 1
  SetCtlColors $IB_Close "${IB_TEXT_DIM}" ${IB_CLEAR}
  ${NSD_OnClick} $IB_Close ${onclick}
!macroend

; ---------------------------------------------------------------------------
; The install page
; ---------------------------------------------------------------------------

; NSIS builds this page from a compiled dialog resource, so its controls cannot
; be replaced — but they can be moved, recoloured, and where they have no place
; here, hidden. The progress bar is kept, because NSIS drives it itself.
;
; The status line is not: it sits on the backdrop, and a control with no
; background of its own never erases what it last drew, so NSIS overwriting it
; per file would pile the names on top of each other. One steady line replaces
; the running commentary.
!macro IB_StyleInstallPage name tagline status
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4

  FindWindow $IB_Dialog "#32770" "" $HWNDPARENT

  ; This page is sized from the dialog resource, not from the area the custom
  ; pages get, so it has to be stretched over the window itself.
  StrCpy $1 ${IB_WIN_W}
  StrCpy $2 ${IB_WIN_H}
  !insertmacro IB_Scale $1
  !insertmacro IB_Scale $2
  System::Call 'user32::SetWindowPos(p $IB_Dialog, p 0, i 0, i 0, i $1, i $2, \
    i ${IB_SWP_NOZORDER})'

  !insertmacro IB_Backdrop
  !insertmacro IB_Masthead "${name}" "${tagline}"

  GetDlgItem $0 $IB_Dialog ${IB_ID_LOG}
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $IB_Dialog ${IB_ID_DETAILS}
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $IB_Dialog ${IB_ID_STATUS}
  ShowWindow $0 ${SW_HIDE}

  !insertmacro IB_Label $IB_Status 40 246 440 22 "${status}" dim

  ; A 6px rail. Clearing the theme is what lets PBM_SETBARCOLOR through.
  GetDlgItem $0 $IB_Dialog ${IB_ID_PROGRESS}
  StrCpy $1 110
  StrCpy $2 284
  StrCpy $3 300
  StrCpy $4 6
  !insertmacro IB_Scale $1
  !insertmacro IB_Scale $2
  !insertmacro IB_Scale $3
  !insertmacro IB_Scale $4
  System::Call 'user32::SetWindowPos(p $0, p 0, i $1, i $2, i $3, i $4, \
    i ${IB_SWP_NOZORDER})'
  System::Call 'uxtheme::SetWindowTheme(p $0, w " ", w " ")'
  SendMessage $0 ${PBM_SETBKCOLOR} 0 ${IB_TRACK_BGR}
  SendMessage $0 ${PBM_SETBARCOLOR} 0 ${IB_ACCENT_BGR}
  !insertmacro IB_Round $0 300 6 3

  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!define MUI_CUSTOMFUNCTION_GUIINIT IB_InitWindow

!endif ; IABENCH_THEME_NSH
