# Live PRD implementation review

```text
status: passed
reviewed_at: 2026-07-22
route: docs/company-os/live-prd.html
```

## Rendering decision

The approved Product Map, Product Journey, and OS Architecture PNGs are
embedded directly and visibly labelled `Approved Expected`. They are not
reimplemented as decorative HTML. HTML owns report navigation, accessible
transcripts, business-line selection, jump contracts, and evidence inspection.

Only real product pages use browser Actual screenshots. Existing Actual or
comparison assets retain their source labels; Expected Organization and
scenario references remain Expected.

## Browser acceptance

- desktop `1536×1024`: overview, journey, architecture;
- tablet `900×1180`: overview, journey, architecture;
- mobile `390×844`: overview, journey, architecture;
- no page-level horizontal overflow or broken referenced image;
- every image-dialog trigger has an accessible name;
- business-line and step selection update the deep link;
- Jump Contract, Objects, Authority, and Evidence tabs render;
- User/Object view changes handoff emphasis;
- image evidence opens and closes through the native dialog;
- reduced-motion and mobile context transformations are present.

Raw captures are intentionally ignored under
`.visual-evidence/company-os-v3/live-prd-v1/final/implemented/`.

## Intentional difference from the Expected images

The Expected images are explanatory artifacts inside the report. The report
shell is not pixel-compared against them because that would compare a container
with its own content. Fidelity is instead proven by exact source embedding,
truth labels, and browser checks. Real product UI fidelity continues to use the
existing Expected-versus-Store-live comparison plates owned by the trademark
native-closure acceptance slice.
