window.BENCHMARK_DATA = {
  "lastUpdate": 1792402995716,
  "repoUrl": "https://github.com/Jotrorox/dodo",
  "entries": {
    "Standard library (Ubuntu 24.04 x86-64, -O3)": [
      {
        "commit": {
          "author": {
            "name": "Johannes Müller",
            "username": "Jotrorox",
            "email": "mail@jotrorox.com"
          },
          "committer": {
            "name": "GitHub",
            "username": "web-flow",
            "email": "noreply@github.com"
          },
          "id": "90b33d2d47b20522755f347d8e312cf4561208cd",
          "message": "Merge pull request #57 from Jotrorox/pl011-uart-driver\n\nAdd reusable PL011 UART support for RP2040 and RP2350",
          "timestamp": "2026-10-19T07:24:53Z",
          "url": "https://github.com/Jotrorox/dodo/commit/90b33d2d47b20522755f347d8e312cf4561208cd"
        },
        "date": 1792402995003,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "json.parse.record -O3",
            "value": 200.020984,
            "range": "197.391 to 202.823",
            "unit": "ns/op"
          },
          {
            "name": "json.decode.derived -O3",
            "value": 592.9735874407751,
            "range": "586.233 to 609.065",
            "unit": "ns/op"
          },
          {
            "name": "json.encode.derived -O3",
            "value": 489.1380635403136,
            "range": "482.536 to 527.465",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.escaped -O3",
            "value": 2803.875834613773,
            "range": "2785.860 to 2845.910",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.array_256 -O3",
            "value": 2599.7835750931013,
            "range": "2573.033 to 2621.191",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.array_4096 -O3",
            "value": 40250.286993690206,
            "range": "40068.192 to 40702.155",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.array_16384 -O3",
            "value": 161022.87520391517,
            "range": "159215.510 to 171479.067",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.object_32 -O3",
            "value": 7406.097736431773,
            "range": "7320.241 to 7973.274",
            "unit": "ns/op"
          },
          {
            "name": "json.parse_indexed.object_32 -O3",
            "value": 2481.594908330012,
            "range": "2451.262 to 2520.677",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.object_256 -O3",
            "value": 458926.14319248823,
            "range": "454416.955 to 469492.927",
            "unit": "ns/op"
          },
          {
            "name": "json.parse_indexed.object_256 -O3",
            "value": 18990.5789,
            "range": "18941.047 to 19138.896",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.object_1024 -O3",
            "value": 7356999.074074074,
            "range": "7189014.370 to 7539120.222",
            "unit": "ns/op"
          },
          {
            "name": "json.parse_indexed.object_1024 -O3",
            "value": 83399.75766484671,
            "range": "81603.677 to 87424.910",
            "unit": "ns/op"
          },
          {
            "name": "json.string.parse.ascii -O3",
            "value": 196.798674,
            "range": "194.703 to 200.474",
            "unit": "ns/op"
          },
          {
            "name": "json.string.equal.ascii -O3",
            "value": 190.79591,
            "range": "187.843 to 199.631",
            "unit": "ns/op"
          },
          {
            "name": "json.string.decoded_len.ascii -O3",
            "value": 1.357894012986264,
            "range": "1.350 to 1.369",
            "unit": "ns/op"
          },
          {
            "name": "json.string.decode.ascii -O3",
            "value": 117.61109213843413,
            "range": "117.235 to 120.548",
            "unit": "ns/op"
          },
          {
            "name": "json.string.parse.utf8 -O3",
            "value": 413.1195183837116,
            "range": "410.587 to 439.063",
            "unit": "ns/op"
          },
          {
            "name": "json.string.equal.utf8 -O3",
            "value": 119.36877629419395,
            "range": "118.789 to 121.592",
            "unit": "ns/op"
          },
          {
            "name": "json.string.decoded_len.utf8 -O3",
            "value": 1.3563621044954828,
            "range": "1.341 to 1.381",
            "unit": "ns/op"
          },
          {
            "name": "json.string.decode.utf8 -O3",
            "value": 81.82073196085557,
            "range": "80.460 to 82.160",
            "unit": "ns/op"
          },
          {
            "name": "json.string.parse.escaped -O3",
            "value": 889.3119717930774,
            "range": "880.906 to 959.820",
            "unit": "ns/op"
          },
          {
            "name": "json.string.equal.escaped -O3",
            "value": 1974.4640144393472,
            "range": "1956.593 to 2000.110",
            "unit": "ns/op"
          },
          {
            "name": "json.string.decoded_len.escaped -O3",
            "value": 1245.1774089750254,
            "range": "1229.448 to 1327.263",
            "unit": "ns/op"
          },
          {
            "name": "json.string.decode.escaped -O3",
            "value": 3021.097260564876,
            "range": "2991.621 to 3079.491",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.fixed_array_16 -O3",
            "value": 191.818298,
            "range": "190.455 to 193.205",
            "unit": "ns/op"
          },
          {
            "name": "json.decode.fixed_array_16 -O3",
            "value": 394.4411694577824,
            "range": "391.217 to 399.337",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.fixed_array_64 -O3",
            "value": 668.8825496987238,
            "range": "661.903 to 710.133",
            "unit": "ns/op"
          },
          {
            "name": "json.decode.fixed_array_64 -O3",
            "value": 1416.2676579398715,
            "range": "1401.057 to 1423.454",
            "unit": "ns/op"
          },
          {
            "name": "json.parse.fixed_array_256 -O3",
            "value": 2681.1744068277267,
            "range": "2654.649 to 2832.777",
            "unit": "ns/op"
          },
          {
            "name": "json.decode.fixed_array_256 -O3",
            "value": 6541.852855759923,
            "range": "6462.284 to 6624.862",
            "unit": "ns/op"
          },
          {
            "name": "json.parse_indexed.struct_16 -O3",
            "value": 934.4762063292178,
            "range": "924.896 to 947.568",
            "unit": "ns/op"
          },
          {
            "name": "json.decode_indexed.struct_16 -O3",
            "value": 1569.2459213586974,
            "range": "1561.512 to 1576.024",
            "unit": "ns/op"
          },
          {
            "name": "json.decode.struct_16 -O3",
            "value": 2670.37222810995,
            "range": "2631.121 to 3027.824",
            "unit": "ns/op"
          },
          {
            "name": "json.parse_indexed.struct_64 -O3",
            "value": 3655.8284568372806,
            "range": "3627.605 to 3709.313",
            "unit": "ns/op"
          },
          {
            "name": "json.decode_indexed.struct_64 -O3",
            "value": 6395.772223818188,
            "range": "6363.450 to 6443.480",
            "unit": "ns/op"
          },
          {
            "name": "json.decode.struct_64 -O3",
            "value": 32205.884448025787,
            "range": "31818.530 to 32430.704",
            "unit": "ns/op"
          },
          {
            "name": "web.router.sorted.8 -O3",
            "value": 34.633691439359616,
            "range": "34.326 to 34.707",
            "unit": "ns/op"
          },
          {
            "name": "web.router.indexed.8 -O3",
            "value": 31.48149845938267,
            "range": "31.344 to 31.859",
            "unit": "ns/op"
          },
          {
            "name": "web.router.unsorted.8 -O3",
            "value": 99.57290723731231,
            "range": "98.502 to 104.604",
            "unit": "ns/op"
          },
          {
            "name": "web.decode_path -O3",
            "value": 21.986316539960786,
            "range": "21.842 to 22.208",
            "unit": "ns/op"
          },
          {
            "name": "web.application -O3",
            "value": 543.1459559025677,
            "range": "536.855 to 604.065",
            "unit": "ns/op"
          },
          {
            "name": "web.router.sorted.64 -O3",
            "value": 52.7740845040488,
            "range": "52.573 to 54.138",
            "unit": "ns/op"
          },
          {
            "name": "web.router.indexed.64 -O3",
            "value": 43.2804612146156,
            "range": "43.112 to 43.480",
            "unit": "ns/op"
          },
          {
            "name": "web.router.unsorted.64 -O3",
            "value": 554.5052725742536,
            "range": "552.225 to 560.993",
            "unit": "ns/op"
          },
          {
            "name": "web.router.sorted.256 -O3",
            "value": 75.31654837362463,
            "range": "74.362 to 76.886",
            "unit": "ns/op"
          },
          {
            "name": "web.router.indexed.256 -O3",
            "value": 48.316583900711905,
            "range": "48.000 to 50.882",
            "unit": "ns/op"
          },
          {
            "name": "web.router.unsorted.256 -O3",
            "value": 2183.5980427614086,
            "range": "2149.429 to 2220.213",
            "unit": "ns/op"
          },
          {
            "name": "web.router.sorted.2048 -O3",
            "value": 84.78164050703079,
            "range": "84.011 to 85.598",
            "unit": "ns/op"
          },
          {
            "name": "web.router.indexed.2048 -O3",
            "value": 90.07465904510384,
            "range": "87.874 to 90.632",
            "unit": "ns/op"
          },
          {
            "name": "web.router.unsorted.2048 -O3",
            "value": 15505.112985086282,
            "range": "15394.146 to 15719.341",
            "unit": "ns/op"
          },
          {
            "name": "web.headers.indexed.32 -O3",
            "value": 2430.0316256574006,
            "range": "2405.447 to 2723.325",
            "unit": "ns/op"
          },
          {
            "name": "web.headers.sequential.32 -O3",
            "value": 140.96448698377495,
            "range": "138.409 to 146.839",
            "unit": "ns/op"
          },
          {
            "name": "web.parameters.uncached -O3",
            "value": 35.92077970953183,
            "range": "35.730 to 37.306",
            "unit": "ns/op"
          },
          {
            "name": "web.parameters.captured -O3",
            "value": 2.9727707443551386,
            "range": "2.952 to 3.236",
            "unit": "ns/op"
          },
          {
            "name": "web.path.checked -O3",
            "value": 60.05437024244533,
            "range": "59.536 to 65.125",
            "unit": "ns/op"
          },
          {
            "name": "web.path.validated -O3",
            "value": 28.120467875000383,
            "range": "27.967 to 30.693",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.scan.8 -O3",
            "value": 106.13283224816072,
            "range": "104.396 to 117.536",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.indexed.8 -O3",
            "value": 101.29763699524685,
            "range": "99.704 to 103.329",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.scan.8 -O3",
            "value": 109.04846501926086,
            "range": "108.242 to 112.534",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.indexed.8 -O3",
            "value": 115.8173898438713,
            "range": "113.293 to 119.004",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.scan.8 -O3",
            "value": 104.69094628727994,
            "range": "102.753 to 108.134",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.indexed.8 -O3",
            "value": 102.83636904689973,
            "range": "99.944 to 105.176",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.scan.8 -O3",
            "value": 103.52516805034746,
            "range": "101.996 to 110.711",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.indexed.8 -O3",
            "value": 100.50425601355762,
            "range": "98.647 to 102.672",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.scan.8 -O3",
            "value": 237.7637193873372,
            "range": "235.398 to 241.882",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.indexed.8 -O3",
            "value": 430.6539056644574,
            "range": "427.082 to 440.888",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.scan.8 -O3",
            "value": 203.1904436151048,
            "range": "201.417 to 208.477",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.indexed.8 -O3",
            "value": 408.04921787447205,
            "range": "404.511 to 412.745",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.scan.8 -O3",
            "value": 236.74109847359043,
            "range": "232.168 to 238.777",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.indexed.8 -O3",
            "value": 427.993326340809,
            "range": "425.468 to 433.521",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.scan.8 -O3",
            "value": 236.33878608182007,
            "range": "232.192 to 237.465",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.indexed.8 -O3",
            "value": 429.7808682007706,
            "range": "422.757 to 456.232",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.scan.8 -O3",
            "value": 323.4517140809734,
            "range": "320.759 to 334.900",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.indexed.8 -O3",
            "value": 660.4299955563107,
            "range": "651.996 to 664.471",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.scan.8 -O3",
            "value": 290.88126702158644,
            "range": "284.489 to 313.806",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.indexed.8 -O3",
            "value": 627.7210981085227,
            "range": "619.090 to 672.760",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.scan.8 -O3",
            "value": 327.73788321442044,
            "range": "322.195 to 354.385",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.indexed.8 -O3",
            "value": 668.8158716495863,
            "range": "656.625 to 679.504",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.scan.8 -O3",
            "value": 325.4489451015861,
            "range": "321.427 to 351.585",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.indexed.8 -O3",
            "value": 662.9267702584095,
            "range": "647.491 to 695.812",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.scan.8 -O3",
            "value": 253.3553078434076,
            "range": "247.220 to 259.175",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.indexed.8 -O3",
            "value": 528.8322925696428,
            "range": "524.837 to 538.402",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.scan.8 -O3",
            "value": 203.24551157713952,
            "range": "199.460 to 228.725",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.indexed.8 -O3",
            "value": 401.5020486859499,
            "range": "396.833 to 433.951",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.scan.8 -O3",
            "value": 250.61708941306614,
            "range": "248.717 to 258.859",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.indexed.8 -O3",
            "value": 532.6310469254822,
            "range": "528.200 to 579.340",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.scan.8 -O3",
            "value": 255.61538374577478,
            "range": "247.152 to 274.221",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.indexed.8 -O3",
            "value": 531.9114450465886,
            "range": "527.162 to 543.363",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.scan.8 -O3",
            "value": 353.3723612923925,
            "range": "339.667 to 358.212",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.indexed.8 -O3",
            "value": 603.3305895713847,
            "range": "595.724 to 612.144",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.scan.8 -O3",
            "value": 202.91399549528728,
            "range": "199.494 to 215.549",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.indexed.8 -O3",
            "value": 413.46851388041165,
            "range": "410.244 to 414.801",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.scan.8 -O3",
            "value": 341.80337542782956,
            "range": "324.965 to 354.728",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.indexed.8 -O3",
            "value": 604.2411709376635,
            "range": "590.078 to 615.563",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.scan.8 -O3",
            "value": 345.490492286497,
            "range": "330.495 to 373.084",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.indexed.8 -O3",
            "value": 589.134141442423,
            "range": "587.745 to 611.088",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.scan.64 -O3",
            "value": 558.4139215249197,
            "range": "553.757 to 607.306",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.indexed.64 -O3",
            "value": 200.35028207216715,
            "range": "199.161 to 204.039",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.scan.64 -O3",
            "value": 561.7820272027988,
            "range": "558.375 to 571.344",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.indexed.64 -O3",
            "value": 246.02416373136407,
            "range": "244.199 to 248.400",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.scan.64 -O3",
            "value": 554.5795454545455,
            "range": "549.688 to 566.224",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.indexed.64 -O3",
            "value": 202.063267,
            "range": "199.191 to 217.652",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.scan.64 -O3",
            "value": 562.0350711866836,
            "range": "553.151 to 571.324",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.indexed.64 -O3",
            "value": 203.12483823735843,
            "range": "198.644 to 218.804",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.scan.64 -O3",
            "value": 1473.981921435289,
            "range": "1444.564 to 1615.639",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.indexed.64 -O3",
            "value": 2375.2986689842373,
            "range": "2341.132 to 2439.174",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.scan.64 -O3",
            "value": 1253.0156769425776,
            "range": "1235.877 to 1265.964",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.indexed.64 -O3",
            "value": 2173.1330639093703,
            "range": "2129.030 to 2239.950",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.scan.64 -O3",
            "value": 1466.784307516451,
            "range": "1436.084 to 1487.143",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.indexed.64 -O3",
            "value": 2358.4811180108118,
            "range": "2324.745 to 2368.118",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.scan.64 -O3",
            "value": 1460.5339804404912,
            "range": "1443.707 to 1512.311",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.indexed.64 -O3",
            "value": 2342.7399781679674,
            "range": "2322.424 to 2363.841",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.scan.64 -O3",
            "value": 2114.1562242287114,
            "range": "2089.407 to 2168.079",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.indexed.64 -O3",
            "value": 3115.4769949506867,
            "range": "3078.011 to 3168.402",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.scan.64 -O3",
            "value": 1803.7938467533388,
            "range": "1769.407 to 1926.468",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.indexed.64 -O3",
            "value": 2901.214557697114,
            "range": "2878.961 to 2972.664",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.scan.64 -O3",
            "value": 2161.6006059622355,
            "range": "2122.732 to 2345.535",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.indexed.64 -O3",
            "value": 3158.853647312715,
            "range": "3133.891 to 3209.384",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.scan.64 -O3",
            "value": 2154.3578851990133,
            "range": "2103.496 to 2194.021",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.indexed.64 -O3",
            "value": 3186.540302866758,
            "range": "3106.274 to 3254.784",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.scan.64 -O3",
            "value": 1505.3157104327572,
            "range": "1497.508 to 1514.602",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.indexed.64 -O3",
            "value": 2675.424418034967,
            "range": "2627.678 to 2699.732",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.scan.64 -O3",
            "value": 1268.817506214837,
            "range": "1244.863 to 1447.898",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.indexed.64 -O3",
            "value": 2095.4863216008075,
            "range": "2082.847 to 2136.897",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.scan.64 -O3",
            "value": 1536.2161390127542,
            "range": "1497.393 to 1561.871",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.indexed.64 -O3",
            "value": 2723.6206715139415,
            "range": "2672.383 to 2796.910",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.scan.64 -O3",
            "value": 1513.8819092398878,
            "range": "1505.299 to 1553.526",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.indexed.64 -O3",
            "value": 2705.025123604244,
            "range": "2685.596 to 2796.482",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.scan.64 -O3",
            "value": 1618.1512198239363,
            "range": "1590.992 to 1776.832",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.indexed.64 -O3",
            "value": 2518.5414043764986,
            "range": "2468.117 to 2541.797",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.scan.64 -O3",
            "value": 1266.0860932438454,
            "range": "1234.894 to 1288.105",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.indexed.64 -O3",
            "value": 2176.915114480535,
            "range": "2149.511 to 2207.625",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.scan.64 -O3",
            "value": 1593.5763957415743,
            "range": "1574.441 to 1633.904",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.indexed.64 -O3",
            "value": 2476.804509002875,
            "range": "2456.089 to 2555.305",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.scan.64 -O3",
            "value": 1600.9637931855332,
            "range": "1568.050 to 1617.463",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.indexed.64 -O3",
            "value": 2498.666334824523,
            "range": "2425.223 to 2564.373",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.scan.256 -O3",
            "value": 2012.8746595400462,
            "range": "1965.653 to 2055.820",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.indexed.256 -O3",
            "value": 239.91493387437413,
            "range": "237.183 to 246.035",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.scan.256 -O3",
            "value": 2016.3600428835196,
            "range": "1991.111 to 2123.794",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.indexed.256 -O3",
            "value": 313.0269159968258,
            "range": "309.357 to 342.299",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.scan.256 -O3",
            "value": 2026.390690264402,
            "range": "1995.377 to 2074.456",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.indexed.256 -O3",
            "value": 244.8985077555024,
            "range": "240.850 to 252.319",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.scan.256 -O3",
            "value": 1944.0293207893983,
            "range": "1907.391 to 1990.713",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.indexed.256 -O3",
            "value": 234.27748633706733,
            "range": "230.080 to 245.488",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.scan.256 -O3",
            "value": 5456.525968831886,
            "range": "5348.854 to 5532.676",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.indexed.256 -O3",
            "value": 10428.713691868017,
            "range": "10359.612 to 11362.639",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.scan.256 -O3",
            "value": 4617.4507608448785,
            "range": "4516.785 to 4641.898",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.indexed.256 -O3",
            "value": 9830.937576439508,
            "range": "9625.292 to 9984.342",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.scan.256 -O3",
            "value": 5290.168909300889,
            "range": "5146.685 to 5455.800",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.indexed.256 -O3",
            "value": 10007.714456205895,
            "range": "9873.252 to 10400.545",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.scan.256 -O3",
            "value": 5426.335442037008,
            "range": "5211.593 to 5985.951",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.indexed.256 -O3",
            "value": 10278.261525739095,
            "range": "9885.435 to 10579.685",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.scan.256 -O3",
            "value": 7927.883642562639,
            "range": "7768.562 to 8103.666",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.indexed.256 -O3",
            "value": 13505.93966571545,
            "range": "13439.165 to 14737.698",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.scan.256 -O3",
            "value": 6957.045703839122,
            "range": "6769.930 to 6987.420",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.indexed.256 -O3",
            "value": 12885.328189566115,
            "range": "12701.317 to 14019.203",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.scan.256 -O3",
            "value": 7984.77220336307,
            "range": "7842.548 to 8024.993",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.indexed.256 -O3",
            "value": 13700.610172926235,
            "range": "13453.073 to 14380.560",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.scan.256 -O3",
            "value": 8030.9322143579375,
            "range": "7827.865 to 8582.251",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.indexed.256 -O3",
            "value": 13692.049242424242,
            "range": "13479.878 to 13957.675",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.scan.256 -O3",
            "value": 5624.256156256995,
            "range": "5527.288 to 5735.082",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.indexed.256 -O3",
            "value": 11365.15833286143,
            "range": "11133.706 to 11418.479",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.scan.256 -O3",
            "value": 4856.964237516869,
            "range": "4553.908 to 5272.051",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.indexed.256 -O3",
            "value": 9908.623424395508,
            "range": "9780.983 to 10018.122",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.scan.256 -O3",
            "value": 5606.470633495768,
            "range": "5553.108 to 5669.090",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.indexed.256 -O3",
            "value": 11548.006011796733,
            "range": "11298.528 to 12331.248",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.scan.256 -O3",
            "value": 5670.632214803079,
            "range": "5580.408 to 5911.343",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.indexed.256 -O3",
            "value": 11546.378524396421,
            "range": "11375.397 to 12438.850",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.scan.256 -O3",
            "value": 5850.170529990843,
            "range": "5736.258 to 5905.118",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.indexed.256 -O3",
            "value": 11006.875489769263,
            "range": "10919.643 to 11142.844",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.scan.256 -O3",
            "value": 4877.6568666043395,
            "range": "4686.549 to 4979.387",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.indexed.256 -O3",
            "value": 9865.948735711812,
            "range": "9767.890 to 10121.197",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.scan.256 -O3",
            "value": 5832.762059003764,
            "range": "5803.209 to 6092.772",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.indexed.256 -O3",
            "value": 11028.306639662816,
            "range": "10895.697 to 11152.306",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.scan.256 -O3",
            "value": 5849.789272142649,
            "range": "5783.379 to 5936.291",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.indexed.256 -O3",
            "value": 11003.62157827794,
            "range": "10863.051 to 12005.084",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.scan.2048 -O3",
            "value": 15312.724056788767,
            "range": "15151.672 to 15333.899",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.hit.indexed.2048 -O3",
            "value": 246.36315312023586,
            "range": "241.095 to 250.712",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.scan.2048 -O3",
            "value": 15279.892227657427,
            "range": "15144.001 to 16711.959",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.miss.indexed.2048 -O3",
            "value": 319.5413634011799,
            "range": "315.625 to 337.314",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.scan.2048 -O3",
            "value": 15331.748678709473,
            "range": "15251.023 to 16952.492",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.head.indexed.2048 -O3",
            "value": 250.01514161288895,
            "range": "243.636 to 252.311",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.scan.2048 -O3",
            "value": 15441.510216623137,
            "range": "15335.024 to 15848.068",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.distinct.method_not_allowed.indexed.2048 -O3",
            "value": 244.56667633583245,
            "range": "242.132 to 248.296",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.scan.2048 -O3",
            "value": 42851.548835125446,
            "range": "42077.283 to 46367.016",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.hit.indexed.2048 -O3",
            "value": 59344.709032068255,
            "range": "59002.694 to 60246.472",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.scan.2048 -O3",
            "value": 37307.66725321133,
            "range": "36929.045 to 37747.645",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.miss.indexed.2048 -O3",
            "value": 54338.84196259149,
            "range": "53292.204 to 54632.255",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.scan.2048 -O3",
            "value": 43311.49544201823,
            "range": "42319.297 to 44480.077",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.head.indexed.2048 -O3",
            "value": 59521.73280342403,
            "range": "58159.102 to 61442.748",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.scan.2048 -O3",
            "value": 42729.980627500525,
            "range": "42268.164 to 43504.762",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.shared.method_not_allowed.indexed.2048 -O3",
            "value": 59738.517118189935,
            "range": "58306.258 to 60160.069",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.scan.2048 -O3",
            "value": 62054.77738515901,
            "range": "60047.865 to 64121.841",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.hit.indexed.2048 -O3",
            "value": 82663.2678798138,
            "range": "79508.798 to 89680.681",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.scan.2048 -O3",
            "value": 55373.824523160765,
            "range": "54633.932 to 59731.102",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.miss.indexed.2048 -O3",
            "value": 77412.18552875696,
            "range": "75690.078 to 83004.374",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.scan.2048 -O3",
            "value": 61861.96875,
            "range": "60603.447 to 62820.826",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.head.indexed.2048 -O3",
            "value": 83534.01071122536,
            "range": "82517.729 to 91451.882",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.scan.2048 -O3",
            "value": 61328.86358136721,
            "range": "59558.338 to 63702.697",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.nested.method_not_allowed.indexed.2048 -O3",
            "value": 81678.42335153297,
            "range": "80359.129 to 94355.134",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.scan.2048 -O3",
            "value": 44610.003619909505,
            "range": "42976.358 to 45067.780",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.hit.indexed.2048 -O3",
            "value": 66770.41910582181,
            "range": "65752.874 to 67735.583",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.scan.2048 -O3",
            "value": 38566.742704480064,
            "range": "37795.112 to 42864.416",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.miss.indexed.2048 -O3",
            "value": 54450.582346609255,
            "range": "54174.657 to 56080.966",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.scan.2048 -O3",
            "value": 44136.463356461405,
            "range": "43739.193 to 45304.909",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.head.indexed.2048 -O3",
            "value": 66022.51717032967,
            "range": "64708.605 to 66820.033",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.scan.2048 -O3",
            "value": 44674.13975955346,
            "range": "44262.448 to 45383.474",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.wildcard.method_not_allowed.indexed.2048 -O3",
            "value": 66161.4588429752,
            "range": "65294.056 to 70196.067",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.scan.2048 -O3",
            "value": 44469.50336215178,
            "range": "44209.606 to 45089.337",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.hit.indexed.2048 -O3",
            "value": 62834.08801441875,
            "range": "61770.894 to 67954.283",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.scan.2048 -O3",
            "value": 38656.73851656626,
            "range": "37792.823 to 38955.575",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.miss.indexed.2048 -O3",
            "value": 54164.148577449945,
            "range": "52557.100 to 55938.277",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.scan.2048 -O3",
            "value": 44889.66206896552,
            "range": "43849.606 to 45470.332",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.head.indexed.2048 -O3",
            "value": 62197.953129724825,
            "range": "60953.520 to 63597.852",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.scan.2048 -O3",
            "value": 44907.47864967583,
            "range": "44191.483 to 49942.688",
            "unit": "ns/op"
          },
          {
            "name": "web.dynamic.mixed.method_not_allowed.indexed.2048 -O3",
            "value": 62357.76856967465,
            "range": "61650.513 to 71757.345",
            "unit": "ns/op"
          },
          {
            "name": "http.serial.c1 -O3",
            "value": 87538.115,
            "range": "86669 to 89067",
            "unit": "ns/request",
            "extra": "p95 latency 104.7 us"
          },
          {
            "name": "http.concurrent.c1 -O3",
            "value": 55659.095,
            "range": "50852 to 58899",
            "unit": "ns/request",
            "extra": "p95 latency 64.2 us"
          },
          {
            "name": "http.concurrent.c8 -O3",
            "value": 67532.843125,
            "range": "65785 to 69257",
            "unit": "ns/request",
            "extra": "p95 latency 1247.9 us"
          }
        ]
      }
    ]
  }
}