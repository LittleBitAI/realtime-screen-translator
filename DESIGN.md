---
name: Manga Reader Workspace
colors: {}
typography:
  page-title:
    fontSize: 28px
    fontWeight: 700
    lineHeight: 1.2
  section-title:
    fontSize: 20px
    fontWeight: 700
    lineHeight: 1.3
  item-title:
    fontSize: 16px
    fontWeight: 600
    lineHeight: 1.5
  body:
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.5
  supporting:
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.5
spacing:
  control-gap: 8px
  phone-gutter: 16px
  desktop-gutter: 32px
  pane-gap: 24px
  touch-target: 48px
rounded:
  control: 8px
  panel: 12px
components:
  workspace:
    maxWidth: 1440px
  list-pane:
    width: 272px
---

# Manga reader workspace

Readers come to open a manga chapter, organize saved titles and return to reading. From source entry they can submit a chapter URL, select a recent URL or import an image folder. The reader toolbar returns them to the app. In the library they can save a URL, find titles by name or address, and reopen a source. The export controls live with the saved pictures. Settings holds the API key and source language.

## Phone

```text
만화 열기                  번역본
[ Chapter URL            ]  [ Title or address    ]
[ Open                   ]  [ Save link ] [ Edit ]
[ Import image folder    ]  Reading              ⌄
Recent URLs                    Title       [ Read ]
Translation status             Export formats    ›
                             Read next            ›
                             All titles           ›
Open | Library | Settings   Open | Library | Settings
```

The selected list expands beneath its button. Each named list shows its saved-title count. Navigation stays at the bottom. The reader's fixed toolbar includes the return action with the translation toggle.

## Windows

```text
Open manga     Library     Settings

Library
[ Search by title or address      ]    [ Save link ] [ Edit ]

Lists                      Reading
All titles                 [ Cover | Title   Read  ] [ Title   Read  ]
Reading                    [ Export formats       ]
Read next
Completed
```

The two panes start at 62rem, where the content area can fit a 272px list pane, a 24px gap and a title pane with enough space for editing. Narrow windows use the single-column layout. The open screen puts URL entry next to its submit button where space permits, with folder import beneath and translation status beside the form. Settings uses the same shell.

## Data and interaction

The installed app reads and writes its device-local library. The browser serves the same screens against its configured data folder. The mounted library owns its results, and its request generation drops results from previous selections or searches. Only explicit actions write to storage. Selecting a list or entering a query changes what the screen reads. A list edit preserves the underlying books and pictures.
