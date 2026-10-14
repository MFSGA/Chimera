# Offline traffic map

`world-map.json` is the generated Natural Earth asset used by the reference
traffic analysis map. Its source data is pinned to commit
`ca96624a56bd078437bca8184e78163e5039ad19`:

- [1:110m country outlines](https://github.com/nvkelso/natural-earth-vector/blob/ca96624a56bd078437bca8184e78163e5039ad19/geojson/ne_110m_admin_0_countries.geojson)
- [1:10m region label points](https://github.com/nvkelso/natural-earth-vector/blob/ca96624a56bd078437bca8184e78163e5039ad19/geojson/ne_10m_admin_0_countries.geojson)

Natural Earth data is [public domain](https://www.naturalearthdata.com/about/terms-of-use/).
The asset stores the projected sphere, graticule, country outlines, unassigned
outlines, and region label points. It contains no server coordinates. The map
is bundled with Chimera; the app does not request map tiles or geolocation data.

This copy follows local reference commit
`fb0039b11a12c0f575fb92c1481f4ebaf66eb1d5`. The reference generator is
`ref/scripts/src/generate/generate-topology-map.ts`, exposed there as
`deno task generate:topology-map`. Regenerate in the reference checkout, review
the source revision and asset diff, then copy the generated JSON here. Chimera
does not yet carry the generator or its Deno dependencies; migrate that tool
when the offline map data needs an independent update path.

Region names and usage come from the backend traffic report. Unknown or
unsupported region codes stay unknown, and proxy names are never used to infer
an exit location. Route lines connect known endpoint regions; they do not
represent measured network hops.
