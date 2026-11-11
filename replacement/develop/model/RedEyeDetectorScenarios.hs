{-# LANGUAGE GHC2021 #-}
module RedEyeDetectorScenarios
  ( redEyeDetectorPublications
  , redEyeDetectorScenarios
  , redEyeDetectorInitial
  , redEyeDetectorRuntime
  ) where

import qualified Data.ByteString as B
import Data.Int (Int16, Int32)
import Data.Word (Word16, Word32)
import PipelineModel
import Scenarios

publication :: Word32 -> String -> StagePublication
publication entry suffix = StagePublication (Address entry) (StageSymbol ("xm1_pipeline_image_redeye_" ++ suffix))

redEyeDetectorPublications :: [StagePublication]
redEyeDetectorPublications = [publication entry suffix | (entry,suffix) <-
  [(0x1075d9c,"detect"),(0x1078f50,"search_local"),(0x1075500,"gradient"),
   (0x10751a4,"remove_duplicates"),(0x107524c,"remove_nearby"),(0x10753b4,"accumulate_scores"),
   (0x10754c0,"filter_candidates"),(0x108678c,"gradient_angle"),(0x1086904,"angle_sine"),(0x1086c40,"angle_cosine")]]

slice :: B.ByteString -> Word32 -> Word32 -> Word32 -> Patch
slice image base start size
  | start < base || fromIntegral (start-base+size) > B.length image = error "red-eye original image extent is missing"
  | otherwise = Patch (Address start) (B.take (fromIntegral size) (B.drop (fromIntegral (start-base)) image))

redEyeDetectorInitial :: B.ByteString -> [Patch]
redEyeDetectorInitial image =
  [slice image 0xbf1a00 0x107bd24 0x7c30,slice image 0xbf1a00 0x1086da0 0x60c]

redEyeDetectorRuntime :: B.ByteString -> [Patch]
redEyeDetectorRuntime image4 = [slice image4 0x61c400 0x722054 292,slice image4 0x61c400 0x7221c8 80]

halfBytes :: Word16 -> B.ByteString
halfBytes n = B.take 2 (wordBytes (fromIntegral n))
halves :: [Int16] -> B.ByteString
halves = B.concat . map (halfBytes . fromIntegral)

candidate :: Int32 -> Int32 -> Int16 -> Int16 -> Int16 -> Int16 -> Int32 -> B.ByteString
candidate x y scale final coarse fine score =
  B.concat [wordBytes 1,wordBytes (fromIntegral x),wordBytes (fromIntegral y),halves [scale,final,coarse,fine],wordBytes (fromIntegral score)]

redEyeDetectorScenarios :: B.ByteString -> [StageScenario]
redEyeDetectorScenarios image = localCases ++ featureCases ++ detectorCases ++ recordCases ++ angleCases ++ gradientCases
 where
  make name entry suffix arguments initial final returned = StageScenario (StageCaseName ("redeye-detector-" ++ name))
    (publication entry suffix) arguments (redEyeDetectorInitial image ++ initial) final returned [] []
  imageDescriptor = 0x1900000
  pixels = 0x1901000
  list = 0x1900100
  records = 0x1904000
  detector = 0x1900200
  faces = 0x1900300
  groups = 0x1900400
  scratch = 0x1905000
  descriptor width height = wordsAt imageDescriptor [pixels,height,width]
  listInitial capacity count = [wordsAt list [records,24,capacity,count],Patch (Address records) (B.replicate 256 0xa5)]
  packed width height value = Patch (Address pixels) (B.concat (replicate (width*height `div` 2) (B.pack [value,value,73,181])))
  table address bytes = Patch (Address address) bytes
  features = B.concat (replicate 100 (halves [0,1,5,5,1]))
  thresholds = B.replicate 200 0
  scoreTable accepted = B.concat [halves [if stage == 0 then if bin == 40 then accepted else -1 else 0 | bin <- [0..63 :: Int]] | stage <- [0..99 :: Int]]
  synthetic =
    [table 0x107bd24 features,table 0x107f404 features,
     table 0x107c10c thresholds,table 0x107f7ec thresholds,table 0x107c1d4 (B.replicate 48 0),
     table 0x107c204 (scoreTable 24),table 0x107f8b4 (scoreTable 1),
     table 0x10836b4 (B.concat (replicate 24 (halves [0,0]))),
     table 0x1082ab4 (B.concat [halves [if stage == 0 then 10 else 0 | _ <- [0..63 :: Int]] | stage <- [0..23 :: Int]])]
  detected x y = candidate x y 1024 10 24 1 10
  localCases =
    [make ("local-positive-capacity-"++show capacity) 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 160] ++ listInitial capacity 0)
      [word (list+12) (min capacity 4),table records (B.concat (take (fromIntegral capacity) [detected 0 0,detected 16 0,detected 0 16,detected 16 16]))] Nothing
    | capacity <- [0,1,2,4]] ++
    [make "local-negative-pixels" 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 0] ++ listInitial 4 0)
      [word (list+12) 0,table records (B.replicate 256 0xa5)] Nothing,
     make "local-edge-ten-pixels" 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 10 10,packed 10 10 160] ++ listInitial 4 0)
      [word (list+12) 0,table records (B.replicate 256 0xa5)] Nothing,
     make "local-append-preserves-existing-record" 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 160] ++ listInitial 2 1 ++ [table records (detected 77 99)])
      [word (list+12) 2,table records (B.concat [detected 77 99,detected 0 0])] Nothing,
     make "local-invalid-feature-aborts" 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 160,table 0x107bd26 (halves [4])] ++ listInitial 4 0)
      [word (list+12) 0,table records (B.replicate 256 0xa5)] Nothing,
     make "local-fine-rejection" 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 160,table 0x107f7ec (halves [2])] ++ listInitial 4 0)
      [word (list+12) 0,table records (B.replicate 256 0xa5)] Nothing,
     make "local-final-rejection" 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 160,table 0x107c1d4 (halves [11])] ++ listInitial 4 0)
      [word (list+12) 0,table records (B.replicate 256 0xa5)] Nothing]
  scoresAt selected accepted = B.concat [halves [if stage == 0 then if bin == selected then accepted else -1 else 0 | bin <- [0..63 :: Int]] | stage <- [0..99 :: Int]]
  featureCases =
    [make ("feature-"++show (component,kind)) 0x1078f50 "search_local" [imageDescriptor,list,1024,0]
      (synthetic ++ [descriptor 12 12,packed 12 12 160,
        table 0x107bd24 (B.concat (replicate 100 (halves [component,kind,5,5,1]))),
        table 0x107f404 (B.concat (replicate 100 (halves [component,kind,5,5,1]))),
        table 0x107c204 (scoresAt bin 24),table 0x107f8b4 (scoresAt bin 1),
        table 0x1083754 (B.pack [if difference < 0 then 0 else if difference == 0 then 3 else 7 | difference <- [-255..255 :: Int]]),
        table 0x1083714 (B.pack [0..63])] ++ listInitial 4 0)
      [word (list+12) 4,table records (B.concat [detected 0 0,detected 16 0,detected 0 16,detected 16 16])] Nothing
    | component <- [0..3],kind <- [0..3],let bin = if kind /= 1 then 27 else case component of 0 -> 40; 1 -> 45; 2 -> 41; _ -> 18]
  config = word 0x96ca50 0x9e0000 : [table (0x9e0000+key) (B.singleton value) | (key,value) <- [(0xa446,1),(0xa447,0),(0xa448,0),(0xa456,1)]]
  faceInitial disabled = [table faces (B.replicate 38 0),table faces (B.singleton 1),table (faces+14) (halves [28]),table (faces+18) (B.singleton disabled)]
  detectorInitial = [wordsAt (detector+44) [scratch,24,4,3],Patch (Address scratch) (B.replicate 96 0xa5),wordsAt groups [0,0,pixels,0x001c000e,0]]
  detectorCases =
    [make ("face-"++name) 0x1075d9c "detect" [imageDescriptor,list,detector,0,0,0,0,0,faces,groups]
      (synthetic ++ config ++ [descriptor 14 28,packed 14 28 value] ++ listInitial capacity 2 ++ faceInitial disabled ++ detectorInitial)
      ([word (list+12) expectedCount,word (detector+56) scratchCount,table 0x14e213a (B.pack [0,1,1,0])] ++ final)
      Nothing
    | (name,value,disabled,capacity,expectedCount,scratchCount,final) <-
        [("positive",160,0,4,1,1,[table records (detected 80 80),table scratch (detected 80 80)]),
         ("negative",0,0,4,0,0,[table records (B.replicate 256 0xa5)]),
         ("disabled",160,1,4,0,0,[table records (B.replicate 256 0xa5)]),
         ("output-full",160,0,0,0,1,[table records (B.replicate 256 0xa5),table scratch (detected 80 80)])]] ++
    [make "invalid-feature-preserves-destination" 0x1075d9c "detect" [imageDescriptor,list,detector,0,0,0,0,0,faces,groups]
      (synthetic ++ config ++ [descriptor 14 28,packed 14 28 160] ++ listInitial 4 2 ++ faceInitial 0 ++ detectorInitial ++ [table 0x107bd26 (halves [4])])
      [word (list+12) 2,word (detector+56) 0,table records (B.replicate 256 0xa5)] Nothing]
  a = candidate 0 0 1024 10 24 1 10
  b = candidate 16 0 1024 20 24 1 20
  c = candidate 1000 0 1024 30 24 1 30
  listOf bytes = listInitial 8 (fromIntegral (length bytes)) ++ [table records (B.concat bytes)]
  recordCases =
    [make "exact-duplicate-stable-order" 0x10751a4 "remove_duplicates" [list]
      (listOf [a,a,b]) [word (list+12) 2,table records (B.concat [a,b])] Nothing,
     make "duplicate-score-is-part-of-key" 0x10751a4 "remove_duplicates" [list]
      (listOf [a,candidate 0 0 1024 11 24 1 11]) [word (list+12) 2,table records (B.concat [a,candidate 0 0 1024 11 24 1 11])] Nothing,
     make "nearby-higher-score-wins" 0x107524c "remove_nearby" [list,11]
      (listOf [a,b,c]) [word (list+12) 2,table records (B.concat [b,c])] Nothing,
     make "nearby-equal-score-preserves-both" 0x107524c "remove_nearby" [list,11]
      (listOf [a,detected 16 0]) [word (list+12) 2,table records (B.concat [a,detected 16 0])] Nothing,
     make "nearby-scale-factor-two-boundary" 0x107524c "remove_nearby" [list,11]
      (listOf [a,candidate 0 0 2049 20 24 1 20]) [word (list+12) 2,table records (B.concat [a,candidate 0 0 2049 20 24 1 20])] Nothing,
     make "neighborhood-scores-directed-pairs" 0x10753b4 "accumulate_scores" [list]
      (listOf [a,b,c]) [word (list+12) 3,table records (B.concat [candidate 0 0 1024 10 24 1 50,candidate 16 0 1024 20 24 1 40,c])] Nothing,
     make "negative-wrapped-distance-aborts-before-compaction" 0x107524c "remove_nearby" [list,11]
      (listOf [a,candidate 50000 0 1024 20 24 1 20]) [word (list+12) 2,table records (B.concat [a,candidate 50000 0 1024 20 24 1 20])] Nothing]
  angleCases =
    [make ("axis-angle-"++show (vertical,horizontal)) 0x108678c "gradient_angle" [fromIntegral vertical,fromIntegral horizontal] [] [] (Just result)
      | (vertical,horizontal,result) <- [(0,0,0),(1,0,2048),(-1,0,0xfffff800),(0,1,0),(0,-1,0xfffff000)] :: [(Int32,Int32,Word32)]] ++
    [make ("sine-quarter-turn-"++show angle) 0x1086904 "angle_sine" [fromIntegral angle] [] [] (Just result)
      | (angle,result) <- [(0,0),(2048,4096),(4096,0),(-2048,0xfffff000),(10240,4096)] :: [(Int32,Word32)]] ++
    [make "cosine-zero" 0x1086c40 "angle_cosine" [0] [] [] (Just 4096)]
  slopePixels descending = table pixels (B.concat [B.pack [fromIntegral (if descending then 220-x*4 else 32+x*4),fromIntegral (if descending then 216-x*4 else 36+x*4),73,181] | _y <- [0..15 :: Int],x <- [0,2..14]])
  gradientCases =
    [make "gradient-nonzero-inward" 0x1075500 "gradient" [pixels,16,16,7,7,0,0] [slopePixels True] [] (Just 255),
     make "gradient-nonzero-outward" 0x1075500 "gradient" [pixels,16,16,7,7,0,0] [slopePixels False] [] (Just 0),
     make "gradient-flat-nonzero-pixels" 0x1075500 "gradient" [pixels,16,16,7,7,0,0] [packed 16 16 160] [] (Just 0)]
