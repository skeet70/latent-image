module Publications (Publication (..), publications) where

import Scenarios (Address (..))

data Publication = Publication Address String deriving (Eq, Show)

publications :: [Publication]
publications = [Publication (Address address) ("xm1_develop_" ++ name) | (address,name) <-
  [(0xea0934,"dispatch"),(0xea17d0,"entry"),(0xe92488,"load"),(0xe92368,"post"),(0xe92328,"initialize"),(0xe99d4c,"save_step"),(0xe9e038,"wait_step")
  ,(0xe91810,"plan_lookup"),(0xe93534,"dispatch_objects"),(0xe93558,"lookup_object"),(0xe93664,"callback"),(0xe936a8,"finish"),(0xe9371c,"current"),(0xe93770,"cancel_all"),(0xe937f8,"cancel")
  ,(0xe9390c,"store"),(0xe93950,"operation_index"),(0xe9399c,"transition_supported"),(0xe93a60,"simple_request"),(0xe93b0c,"followup"),(0xe93bdc,"transition"),(0xe93c34,"begin"),(0xe93d0c,"complete"),(0xe92918,"precondition"),(0xe92ad8,"transform_required"),(0xe9db80,"diagnostic"),(0xe91d50,"finish_trace")
  ,(0xe67750,"permit_request"),(0xe7ee84,"decode"),(0xe6a74c,"permit_primary"),(0xe6bb88,"permit_secondary"),(0xe93760,"current")]]
  ++ [Publication (Address address) name | (address,name) <- [(0xe518c0,"xm1_save_facade"),(0xe4e5dc,"xm1_save_completion"),(0xe4e5a8,"xm1_save_release_completion"),(0xe77168,"rb_media_permit_resolve"),(0xe7e95c,"rb_media_permit_apply")]]
