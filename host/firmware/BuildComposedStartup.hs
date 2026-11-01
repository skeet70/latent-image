{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (unless)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import FujiAF.FirstStage
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import qualified StartupStageContract as Stage
import System.Environment (getArgs)

data Composition = Ordinary | InterleavedTail

data Options = Options FilePath FilePath FilePath FilePath FilePath FilePath Sha256 Sha256 Sha256 Composition

usage :: String
usage = "usage: build-composed-startup SOURCE_DAT FIRST_BIN FIRST_SYMBOLS SECOND_PREFIX_BIN SECOND_PREFIX_SYMBOLS OUT_DAT --expect-source-sha256 HEX --expect-first-slot-sha256 HEX --expect-second-slot-sha256 HEX [--negative-interleaved-tail]"

parseOptions :: [String] -> Either String Options
parseOptions (source : first : firstSymbols : second : secondSymbols : output : flags) = do
  let negativeFlags = filter (== "--negative-interleaved-tail") flags
      mode = if null negativeFlags then Ordinary else InterleavedTail
  unless (length negativeFlags <= 1) (Left "duplicate option: --negative-interleaved-tail")
  values <- collect [] (filter (/= "--negative-interleaved-tail") flags)
  sourceDigest <- need "--expect-source-sha256" values
  firstDigest <- need "--expect-first-slot-sha256" values
  secondDigest <- need "--expect-second-slot-sha256" values
  pure (Options source first firstSymbols second secondSymbols output sourceDigest firstDigest secondDigest mode)
  where
    collect values [] = Right values
    collect values (flag : value : rest)
      | flag `elem` ["--expect-source-sha256", "--expect-first-slot-sha256", "--expect-second-slot-sha256"] = do
          unless (flag `notElem` map fst values) (Left ("duplicate option: " ++ flag))
          digest <- parseSha256 value
          collect ((flag, digest) : values) rest
    collect _ _ = Left usage
    need flag values = maybe (Left (flag ++ " is required")) Right (lookup flag values)
parseOptions _ = Left usage

digestOf :: B.ByteString -> IO Sha256
digestOf bytes = H.sha256 bytes >>= either H.failH pure . parseSha256

checkDigest :: String -> Sha256 -> B.ByteString -> IO ()
checkDigest label expected bytes = do
  actual <- digestOf bytes
  H.ensure (actual == expected) (label ++ " digest differs: " ++ sha256Hex actual)

imageIdentity :: StagePlan -> Sha256 -> Sha256 -> Value
imageIdentity plan image symbols = object
  [ "record" .= (case planRecord plan of FirstRecord -> (1 :: Int); SecondRecord -> 2)
  , "start" .= start
  , "entry" .= armEntryAddress (planEntry plan)
  , "end" .= (start + B.length (U.mediaImageBytes (planImage plan)))
  , "image_sha256" .= sha256Hex image
  , "symbols_sha256" .= sha256Hex symbols
  , "retained" .= map retainedExtentRange (planRetainedExtents plan)
  ]
  where
    start = recordGuestBase (planRecord plan) + slotOffsetBytes (planLoadOffset plan)

main :: IO ()
main = do
  options <- getArgs >>= either H.failH pure . parseOptions
  let Options sourcePath firstPath firstSymbolsPath secondPath secondSymbolsPath output expectedSource expectedFirst expectedSecond mode = options
  sourceBytes <- H.readRegular (64 * 1024 * 1024) sourcePath
  original <- either H.failH pure (Stage.parseOriginalStartupDat sourceBytes)
  H.ensure (Stage.originalStartupDigest original == expectedSource) "source DAT digest differs"
  let source = Stage.originalStartupParsed original
  firstSlot <- either H.failH pure (recordSlotBytes FirstRecord (U.parsedParts source))
  secondSlot <- either H.failH pure (recordSlotBytes SecondRecord (U.parsedParts source))
  checkDigest "source first record" expectedFirst firstSlot
  checkDigest "source second record" expectedSecond secondSlot
  first <- H.readRegular firstRecordSlotLength firstPath
  second <- H.readRegular firstRecordSlotLength secondPath
  firstSymbolsBytes <- H.readRegular 0x10000 firstSymbolsPath
  secondSymbolsBytes <- H.readRegular 0x10000 secondSymbolsPath
  firstArtifact <- either H.failH pure (Stage.parseFirstStageArtifact (U.MediaImage first) firstSymbolsBytes)
  secondArtifact <- either H.failH pure (Stage.parseSecondaryPrefixArtifact (U.MediaImage second) secondSymbolsBytes)
  hook <- case mode of
    Ordinary -> do
      H.ensure (all ((/= "xm1_first_stage_interleaved_tail_fault") . symbolName) (Stage.firstStageArtifactSymbols firstArtifact))
        "interleaved-tail fault image requires --negative-interleaved-tail"
      pure Nothing
    InterleavedTail -> Just <$> either H.failH pure (Stage.interleavedTailHook firstArtifact)
  let artifacts = Stage.ComposedArtifacts
        { Stage.firstStageArtifact = firstArtifact
        , Stage.secondaryPrefixArtifact = secondArtifact
        }
  plan <- either H.failH pure (Stage.composedPlans artifacts)
  slotted <- either H.failH pure (replaceComposedRecords plan (U.parsedParts source))
  parts <- either H.failH pure (maybe (Right slotted) (\selected -> Stage.applyInterleavedTailHook selected slotted) hook)
  finalBytes <- either H.failH pure (U.rebuildContainer source parts)
  final <- either H.failH pure (Stage.parseStagedStartupDat finalBytes)
  either H.failH pure $ case mode of
    Ordinary -> verifyComposedRecords plan source (Stage.stagedStartupParsed final)
    InterleavedTail -> Stage.verifyInterleavedTailComposed artifacts original final
  let finalDigest = Stage.stagedStartupDigest final
      publication = composedPublication plan
      manifest = object $
        [ "schema" .= (1 :: Int)
        , "role" .= ("composed-startup" :: String)
        , "source_dat_sha256" .= sha256Hex expectedSource
        , "source_first_slot_sha256" .= sha256Hex expectedFirst
        , "source_second_slot_sha256" .= sha256Hex expectedSecond
        , "final_dat_sha256" .= sha256Hex finalDigest
        , "first_stage" .= imageIdentity (composedFirstPlan plan) (Stage.firstStageArtifactImageDigest firstArtifact) (Stage.firstStageArtifactSymbolsDigest firstArtifact)
        , "secondary_prefix" .= imageIdentity (composedSecondPlan plan) (Stage.secondaryPrefixArtifactImageDigest secondArtifact) (Stage.secondaryPrefixArtifactSymbolsDigest secondArtifact)
        , "loader_b_publication" .= object
            [ "address" .= loaderPatchAddress publication
            , "expected" .= loaderPatchExpected publication
            , "replacement" .= loaderPatchReplacement publication
            ]
        ]
        ++ case hook of
          Nothing -> []
          Just selected ->
            [ "negative_control" .= ("interleaved-tail" :: String)
            , "interleaved_tail" .= object
                [ "site" .= Stage.interleavedTailSite selected
                , "instruction" .= Stage.interleavedTailInstruction selected
                , "target" .= Stage.interleavedTailTarget selected
                ]
            ]
  B.writeFile output finalBytes
  BL.writeFile (output ++ ".staging.json") (encode manifest)
  putStrLn ("composed startup DAT sha256 " ++ sha256Hex finalDigest)
