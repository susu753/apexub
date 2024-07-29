//Date 7/29/2024
//GameVersion=v3.0.75.30

#define OFFSET_ITEM_ID 0x1568 // item id?      //updated 11/1/2023  [RecvTable.DT_OverlayVars]
#define OFFSET_M_CUSTOMSCRIPTINT 0x1568 //m_customScriptInt //updated 1/10/2024
#define OFFSET_YAW 0x224c - 0x8 //m_currentFramePlayer.m_ammoPoolCount//updated 7/29/2024 - 0x8 

#define HIGHLIGHT_SETTINGS 0xb0cf370 // HighlightSettings  // updated 7/29/2024
#define OFFSET_GLOW_HIGHLIGHT_ID 0x29c // updated 7/29/2024     0x28c
#define OFFSET_GLOW_THROUGH_WALLS 0x26c // updated 1/25/2024
#define OFFSET_GLOW_FIX 0x268 // updated 1/25/2024
#define OFFSET_GLOW_ENABLE  0x26c
#define OFFSET_GLOW_DISTANCE  0x264
#define OFFSET_HIGHLIGHTSIZE  0x34
// Mode: HighlightSettings + 0x34 * Context + 0x0
// Color: HighlightSettings + 0x34 * Context + 0x4
#define OFFSET_GRADE  0x0348   //m_grade
#define m_lastChargeLevel 0x16f0 // m_lastChargeLevel  7/29/2024
